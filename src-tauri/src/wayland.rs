#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaylandCompatDecision {
    pub apply: bool,
    pub disable_dmabuf_renderer: bool,
    pub remove_gbm_backend: bool,
}

pub fn decide_wayland_compat(
    disabled: bool,
    has_wayland_display: bool,
    session_type: Option<&str>,
    dmabuf_override_present: bool,
    keep_gbm_backend: bool,
) -> WaylandCompatDecision {
    let is_wayland = has_wayland_display
        || session_type.is_some_and(|value| value.eq_ignore_ascii_case("wayland"));
    let apply = !disabled && is_wayland;
    WaylandCompatDecision {
        apply,
        disable_dmabuf_renderer: apply && !dmabuf_override_present,
        remove_gbm_backend: apply && !keep_gbm_backend,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_wayland_from_display_or_session_type() {
        assert!(decide_wayland_compat(false, true, None, false, false).apply);
        assert!(decide_wayland_compat(false, false, Some("WayLand"), false, false).apply);
        assert!(!decide_wayland_compat(false, false, Some("x11"), false, false).apply);
    }

    #[test]
    fn honors_all_user_overrides() {
        let disabled = decide_wayland_compat(true, true, None, false, false);
        assert!(!disabled.apply);
        assert!(!disabled.disable_dmabuf_renderer);
        assert!(!disabled.remove_gbm_backend);

        let overrides = decide_wayland_compat(false, true, None, true, true);
        assert!(overrides.apply);
        assert!(!overrides.disable_dmabuf_renderer);
        assert!(!overrides.remove_gbm_backend);
    }
}
