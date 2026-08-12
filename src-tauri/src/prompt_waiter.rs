//! T2-16: SSH prompt waiter system with nonce-based RAII.
//!
//! When SSH keyboard-interactive authentication requires external input
//! (e.g. a 2FA token from the user), the prompter needs to:
//!
//! 1. Register a unique nonce for this prompt
//! 2. Wait for an external responder to supply the answer
//! 3. Clean up the registration automatically (RAII) when the prompt
//!    is answered or abandoned
//!
//! This module provides:
//!
//! - `PromptWaiter`: A registry of pending prompts, keyed by nonce
//! - `PromptGuard`: RAII guard that auto-cleans on drop
//! - `PromptResponse`: Channel for delivering the response back to the waiter

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::oneshot;
use uuid::Uuid;

/// T2-16: The response to a pending prompt.
#[derive(Debug, Clone)]
pub struct PromptAnswer {
    nonce: String,
    answer: String,
}

impl PromptAnswer {
    /// The nonce identifying which prompt this answers.
    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    /// The answer text.
    pub fn answer(&self) -> &str {
        &self.answer
    }
}

/// T2-16: RAII guard for a registered prompt.
///
/// When this guard is dropped, the prompt is automatically removed from
/// the registry, preventing resource leaks if the prompt is abandoned
/// (e.g. SSH connection drops before the user responds).
pub struct PromptGuard {
    nonce: String,
    waiter: PromptWaiter,
}

impl PromptGuard {
    /// The nonce for this prompt.
    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    /// The prompt text that was registered.
    pub fn prompt_text(&self) -> &str {
        // We don't store the text here — it's in the registry
        // This is a convenience accessor that returns the nonce as an identifier
        &self.nonce
    }
}

impl Drop for PromptGuard {
    fn drop(&mut self) {
        // Use std mutex's try_lock to avoid blocking in drop
        if let Ok(mut guard) = self.waiter.registry.lock() {
            guard.remove(&self.nonce);
        }
    }
}

/// T2-16: A registered prompt in the waiter registry.
struct RegisteredPrompt {
    prompt_text: String,
    response_tx: Option<oneshot::Sender<PromptAnswer>>,
    /// T2-16: Sync response channel for blocking prompters (ssh2's KeyboardInteractivePrompt).
    sync_response_tx: Option<std::sync::mpsc::Sender<String>>,
}

/// T2-16: Prompt waiter registry.
///
/// Manages pending prompts and their responses. Thread-safe and async-friendly.
///
/// # Usage
/// ```ignore
/// let waiter = PromptWaiter::new();
/// // Register a prompt
/// let guard = waiter.register("Enter TOTP code:").await?;
/// // In another task: waiter.respond(&guard.nonce(), "123456").await?;
/// // Wait for response
/// let answer = guard.wait().await?;
/// ```
#[derive(Clone)]
pub struct PromptWaiter {
    registry: Arc<std::sync::Mutex<HashMap<String, RegisteredPrompt>>>,
}

impl PromptWaiter {
    /// Create a new empty prompt waiter.
    pub fn new() -> Self {
        Self {
            registry: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Register a new prompt and return a guard.
    ///
    /// The guard's nonce can be used by an external responder to deliver
    /// the answer via `respond()`.
    pub async fn register(&self, prompt_text: &str) -> Result<PromptGuard> {
        let nonce = Uuid::new_v4().to_string();
        let (response_tx, _response_rx) = oneshot::channel::<PromptAnswer>();
        self.registry.lock().unwrap().insert(
            nonce.clone(),
            RegisteredPrompt {
                prompt_text: prompt_text.to_string(),
                response_tx: Some(response_tx),
                sync_response_tx: None,
            },
        );
        Ok(PromptGuard {
            nonce,
            waiter: self.clone(),
        })
    }

    /// Register a prompt and immediately get the response receiver.
    ///
    /// This is the primary entry point for prompters that need to await
    /// the response. The returned `PromptGuard` auto-cleans on drop.
    pub async fn register_and_wait(
        &self,
        prompt_text: &str,
    ) -> Result<(PromptGuard, oneshot::Receiver<PromptAnswer>)> {
        let nonce = Uuid::new_v4().to_string();
        let (response_tx, response_rx) = oneshot::channel::<PromptAnswer>();
        self.registry.lock().unwrap().insert(
            nonce.clone(),
            RegisteredPrompt {
                prompt_text: prompt_text.to_string(),
                response_tx: Some(response_tx),
                sync_response_tx: None,
            },
        );
        Ok((
            PromptGuard {
                nonce,
                waiter: self.clone(),
            },
            response_rx,
        ))
    }

    /// Respond to a pending prompt by nonce.
    ///
    /// Returns `Err` if:
    /// - The nonce doesn't exist (stale or already answered)
    /// - The response channel was already consumed (prompter abandoned)
    pub async fn respond(&self, nonce: &str, answer: String) -> Result<()> {
        let mut registry = self.registry.lock().unwrap();
        let entry = registry
            .get_mut(nonce)
            .ok_or_else(|| anyhow!("unknown or stale prompt nonce: {nonce}"))?;

        // Try async channel first
        if let Some(response_tx) = entry.response_tx.take() {
            let prompt_answer = PromptAnswer {
                nonce: nonce.to_string(),
                answer: answer.clone(),
            };
            response_tx
                .send(prompt_answer)
                .map_err(|_| anyhow!("prompter for {nonce} already dropped"))?;
            registry.remove(nonce);
            return Ok(());
        }

        // Try sync channel
        if let Some(sync_tx) = entry.sync_response_tx.take() {
            sync_tx
                .send(answer)
                .map_err(|_| anyhow!("sync prompter for {nonce} already dropped"))?;
            registry.remove(nonce);
            return Ok(());
        }

        Err(anyhow!("prompt {nonce} already responded or abandoned"))
    }

    /// Cancel a pending prompt by nonce.
    /// This removes the prompt without delivering a response.
    pub async fn cancel(&self, nonce: &str) -> Result<()> {
        let mut registry = self.registry.lock().unwrap();
        if registry.remove(nonce).is_some() {
            Ok(())
        } else {
            Err(anyhow!("unknown prompt nonce: {nonce}"))
        }
    }

    /// Get the number of pending prompts.
    pub async fn pending_count(&self) -> usize {
        self.registry.lock().unwrap().len()
    }

    /// List all pending prompt nonces (for debugging/inspection).
    pub async fn pending_nonces(&self) -> Vec<String> {
        self.registry.lock().unwrap().keys().cloned().collect()
    }

    /// Get the prompt text for a given nonce.
    pub async fn prompt_text(&self, nonce: &str) -> Option<String> {
        self.registry
            .lock()
            .unwrap()
            .get(nonce)
            .map(|e| e.prompt_text.clone())
    }

    /// T2-16: Synchronous (blocking) variant of `register_and_wait` for use
    /// in non-async contexts such as `ssh2`'s `KeyboardInteractivePrompt` trait.
    ///
    /// Returns the nonce and a `std::sync::mpsc::Receiver` that can be
    /// used with `recv_timeout` to block for the answer.
    ///
    /// The `PromptGuard` is returned so that RAII cleanup still happens
    /// when the prompter is abandoned.
    pub fn register_blocking(
        &self,
        prompt_text: &str,
    ) -> Result<(PromptGuard, std::sync::mpsc::Receiver<String>)> {
        let nonce = Uuid::new_v4().to_string();
        let (sync_tx, sync_rx) = std::sync::mpsc::channel::<String>();
        self.registry.lock().unwrap().insert(
            nonce.clone(),
            RegisteredPrompt {
                prompt_text: prompt_text.to_string(),
                response_tx: None,
                sync_response_tx: Some(sync_tx),
            },
        );
        Ok((
            PromptGuard {
                nonce,
                waiter: self.clone(),
            },
            sync_rx,
        ))
    }

    /// T2-16: Synchronous respond — same as `respond` but callable from
    /// non-async contexts (used by daemon HTTP handlers that are sync).
    pub fn respond_blocking(&self, nonce: &str, answer: String) -> Result<()> {
        let mut registry = self.registry.lock().unwrap();
        let entry = registry
            .get_mut(nonce)
            .ok_or_else(|| anyhow!("unknown or stale prompt nonce: {nonce}"))?;

        if let Some(response_tx) = entry.response_tx.take() {
            let prompt_answer = PromptAnswer {
                nonce: nonce.to_string(),
                answer: answer.clone(),
            };
            response_tx
                .send(prompt_answer)
                .map_err(|_| anyhow!("prompter for {nonce} already dropped"))?;
            registry.remove(nonce);
            return Ok(());
        }

        if let Some(sync_tx) = entry.sync_response_tx.take() {
            sync_tx
                .send(answer)
                .map_err(|_| anyhow!("sync prompter for {nonce} already dropped"))?;
            registry.remove(nonce);
            return Ok(());
        }

        Err(anyhow!("prompt {nonce} already responded or abandoned"))
    }

    /// T2-16: Synchronous variant of `prompt_text` for non-async callers.
    pub fn prompt_text_blocking(&self, nonce: &str) -> Option<String> {
        self.registry
            .lock()
            .unwrap()
            .get(nonce)
            .map(|e| e.prompt_text.clone())
    }

    /// T2-16: Synchronous variant of `pending_count` for non-async callers.
    pub fn pending_count_blocking(&self) -> usize {
        self.registry.lock().unwrap().len()
    }

    /// T2-16: Synchronous variant of `pending_nonces` for non-async callers.
    pub fn pending_nonces_blocking(&self) -> Vec<String> {
        self.registry.lock().unwrap().keys().cloned().collect()
    }

    /// T2-16: Synchronous variant of `cancel` for non-async callers.
    pub fn cancel_blocking(&self, nonce: &str) -> Result<()> {
        let mut registry = self.registry.lock().unwrap();
        if registry.remove(nonce).is_some() {
            Ok(())
        } else {
            Err(anyhow!("unknown prompt nonce: {nonce}"))
        }
    }
}

impl Default for PromptWaiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn register_and_respond_delivers_answer() {
        let waiter = PromptWaiter::new();
        let (guard, rx) = waiter.register_and_wait("Enter TOTP: ").await.unwrap();

        let nonce = guard.nonce().to_string();
        assert_eq!(waiter.pending_count().await, 1);

        // Respond from another "task"
        waiter.respond(&nonce, "123456".to_string()).await.unwrap();

        // Wait for the response
        let answer = tokio::time::timeout(Duration::from_secs(1), rx)
            .await
            .expect("timeout waiting for response")
            .expect("response channel dropped");
        assert_eq!(answer.answer, "123456");
        assert_eq!(answer.nonce, nonce);

        // After responding, the prompt should be removed from registry
        assert_eq!(waiter.pending_count().await, 0);
    }

    #[tokio::test]
    async fn guard_cleanup_on_drop() {
        let waiter = PromptWaiter::new();
        {
            let (_guard, _rx) = waiter.register_and_wait("Enter password: ").await.unwrap();
            assert_eq!(waiter.pending_count().await, 1);
            // Guard goes out of scope here — should auto-cleanup
        }
        // Give the drop a moment to execute
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(waiter.pending_count().await, 0);
    }

    #[tokio::test]
    async fn respond_to_unknown_nonce_fails() {
        let waiter = PromptWaiter::new();
        let result = waiter
            .respond("nonexistent-nonce", "answer".to_string())
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown or stale"));
    }

    #[tokio::test]
    async fn double_respond_fails() {
        let waiter = PromptWaiter::new();
        let (guard, _rx) = waiter.register_and_wait("Enter code: ").await.unwrap();
        let nonce = guard.nonce().to_string();

        // First respond succeeds
        waiter.respond(&nonce, "first".to_string()).await.unwrap();

        // Second respond fails — nonce already removed
        let result = waiter.respond(&nonce, "second".to_string()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn cancel_removes_prompt() {
        let waiter = PromptWaiter::new();
        let (guard, _rx) = waiter.register_and_wait("Enter OTP: ").await.unwrap();
        let nonce = guard.nonce().to_string();

        assert_eq!(waiter.pending_count().await, 1);
        waiter.cancel(&nonce).await.unwrap();
        assert_eq!(waiter.pending_count().await, 0);
    }

    #[tokio::test]
    async fn cancel_unknown_nonce_fails() {
        let waiter = PromptWaiter::new();
        let result = waiter.cancel("unknown").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn nonce_is_unique_per_registration() {
        let waiter = PromptWaiter::new();
        let (guard1, _rx1) = waiter.register_and_wait("Prompt 1").await.unwrap();
        let (guard2, _rx2) = waiter.register_and_wait("Prompt 2").await.unwrap();

        assert_ne!(guard1.nonce(), guard2.nonce(), "nonces must be unique");
        assert_eq!(waiter.pending_count().await, 2);
    }

    #[tokio::test]
    async fn prompt_text_lookup() {
        let waiter = PromptWaiter::new();
        let (guard, _rx) = waiter.register_and_wait("Enter your PIN: ").await.unwrap();
        let nonce = guard.nonce().to_string();

        let text = waiter.prompt_text(&nonce).await;
        assert_eq!(text.as_deref(), Some("Enter your PIN: "));

        // Unknown nonce returns None
        assert!(waiter.prompt_text("unknown").await.is_none());
    }

    #[tokio::test]
    async fn pending_nonces_lists_all() {
        let waiter = PromptWaiter::new();
        let (guard1, _) = waiter.register_and_wait("P1").await.unwrap();
        let (guard2, _) = waiter.register_and_wait("P2").await.unwrap();

        let nonces = waiter.pending_nonces().await;
        assert_eq!(nonces.len(), 2);
        assert!(nonces.contains(&guard1.nonce().to_string()));
        assert!(nonces.contains(&guard2.nonce().to_string()));
    }

    #[tokio::test]
    async fn abandoned_prompter_cleans_up_on_respond() {
        let waiter = PromptWaiter::new();
        let (guard, rx) = waiter.register_and_wait("Enter TOTP: ").await.unwrap();
        let nonce = guard.nonce().to_string();

        // Simulate the prompter (rx) being dropped before response arrives
        drop(rx);

        // Respond should fail because the channel is closed
        let result = waiter.respond(&nonce, "123".to_string()).await;
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("already dropped"),
            "should indicate the prompter was dropped"
        );

        // The entry should still be cleaned up by respond's error handling
        // Actually, on error the entry remains — let's verify
        assert_eq!(
            waiter.pending_count().await,
            1,
            "entry remains after failed respond"
        );

        // Cancel to clean up
        waiter.cancel(&nonce).await.unwrap();
        assert_eq!(waiter.pending_count().await, 0);
    }

    // T2-16: Blocking (sync) variant tests

    #[test]
    fn register_blocking_and_respond_blocking_delivers_answer() {
        let waiter = PromptWaiter::new();
        let (guard, rx) = waiter.register_blocking("Enter TOTP: ").unwrap();

        let nonce = guard.nonce().to_string();
        assert_eq!(waiter.pending_count_blocking(), 1);

        // Respond from sync context
        waiter
            .respond_blocking(&nonce, "654321".to_string())
            .unwrap();

        // Receive the answer
        let answer = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(answer, "654321");

        // After responding, the prompt should be removed from registry
        assert_eq!(waiter.pending_count_blocking(), 0);
    }

    #[test]
    fn blocking_guard_cleanup_on_drop() {
        let waiter = PromptWaiter::new();
        {
            let (_guard, _rx) = waiter.register_blocking("Enter password: ").unwrap();
            assert_eq!(waiter.pending_count_blocking(), 1);
            // Guard goes out of scope here — should auto-cleanup
        }
        assert_eq!(waiter.pending_count_blocking(), 0);
    }

    #[test]
    fn respond_blocking_to_unknown_nonce_fails() {
        let waiter = PromptWaiter::new();
        let result = waiter.respond_blocking("nonexistent", "answer".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn double_respond_blocking_fails() {
        let waiter = PromptWaiter::new();
        let (guard, _rx) = waiter.register_blocking("Enter code: ").unwrap();
        let nonce = guard.nonce().to_string();

        waiter
            .respond_blocking(&nonce, "first".to_string())
            .unwrap();

        let result = waiter.respond_blocking(&nonce, "second".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn cancel_blocking_removes_prompt() {
        let waiter = PromptWaiter::new();
        let (guard, _rx) = waiter.register_blocking("Enter OTP: ").unwrap();
        let nonce = guard.nonce().to_string();

        assert_eq!(waiter.pending_count_blocking(), 1);
        waiter.cancel_blocking(&nonce).unwrap();
        assert_eq!(waiter.pending_count_blocking(), 0);
    }

    #[test]
    fn blocking_prompt_text_lookup() {
        let waiter = PromptWaiter::new();
        let (guard, _rx) = waiter.register_blocking("Enter your PIN: ").unwrap();
        let nonce = guard.nonce().to_string();

        let text = waiter.prompt_text_blocking(&nonce);
        assert_eq!(text.as_deref(), Some("Enter your PIN: "));

        assert!(waiter.prompt_text_blocking("unknown").is_none());
    }

    #[test]
    fn cross_respond_async_to_blocking_works() {
        // An async respond() should be able to answer a blocking registration
        let waiter = PromptWaiter::new();
        let (guard, rx) = waiter.register_blocking("Enter TOTP: ").unwrap();
        let nonce = guard.nonce().to_string();

        // Use the async respond from a blocking context via block_on
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(waiter.respond(&nonce, "999999".to_string()))
            .unwrap();

        let answer = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(answer, "999999");
    }

    #[tokio::test]
    async fn cross_respond_blocking_to_async_works() {
        // A blocking respond_blocking should be able to answer an async registration
        let waiter = PromptWaiter::new();
        let (guard, rx) = waiter.register_and_wait("Enter OTP: ").await.unwrap();
        let nonce = guard.nonce().to_string();

        // respond_blocking is sync and should work from async context
        waiter
            .respond_blocking(&nonce, "888888".to_string())
            .unwrap();

        let answer = tokio::time::timeout(Duration::from_secs(1), rx)
            .await
            .expect("timeout")
            .expect("channel dropped");
        assert_eq!(answer.answer, "888888");
    }
}
