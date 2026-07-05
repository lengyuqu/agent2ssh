import * as monaco from "monaco-editor";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import { loader } from "@monaco-editor/react";

// V3-1: the app's CSP is `script-src 'self'` (see src-tauri/tauri.conf.json), so
// @monaco-editor/react's default CDN loader would be blocked. Point it at the
// monaco-editor package bundled by Vite instead.
//
// Importing monaco-editor's main entry registers every bundled language,
// including JSON/CSS/TypeScript, whose language services normally run their
// own worker for on-open validation/diagnostics — the JSON and TS workers
// alone add several MB (the TS worker ships a full compiler). This is a
// read-only preview with no completions/formatting, so there's nothing for
// those services to do; disable their diagnostics instead of shipping their
// workers, and route everything through the one generic editor worker.
let configured = false;

export function ensureMonacoConfigured() {
  if (configured) return;
  configured = true;

  monaco.languages.json.jsonDefaults.setDiagnosticsOptions({ validate: false });
  monaco.languages.css.cssDefaults.setDiagnosticsOptions({ validate: false });
  monaco.languages.css.scssDefaults.setDiagnosticsOptions({ validate: false });
  monaco.languages.css.lessDefaults.setDiagnosticsOptions({ validate: false });
  monaco.languages.typescript.typescriptDefaults.setDiagnosticsOptions({
    noSemanticValidation: true,
    noSyntaxValidation: true,
    noSuggestionDiagnostics: true,
  });
  monaco.languages.typescript.javascriptDefaults.setDiagnosticsOptions({
    noSemanticValidation: true,
    noSyntaxValidation: true,
    noSuggestionDiagnostics: true,
  });

  // monaco-editor's own `declare var MonacoEnvironment` lives in a module-scoped
  // .d.ts (not `declare global`), so it isn't visible as a bare identifier here —
  // assign through a typed cast instead of fighting the ambient declaration.
  (globalThis as unknown as { MonacoEnvironment: monaco.Environment }).MonacoEnvironment = {
    getWorker: () => new EditorWorker(),
  };
  loader.config({ monaco });
}
