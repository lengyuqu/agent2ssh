// V3-1: file-extension → Monaco language id, for SFTP panel text preview.
const EXT_LANGUAGE: Record<string, string> = {
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  ts: "typescript",
  tsx: "typescript",
  json: "json",
  py: "python",
  rs: "rust",
  go: "go",
  java: "java",
  kt: "kotlin",
  c: "c",
  h: "c",
  cpp: "cpp",
  cc: "cpp",
  cxx: "cpp",
  hpp: "cpp",
  cs: "csharp",
  rb: "ruby",
  php: "php",
  sh: "shell",
  bash: "shell",
  zsh: "shell",
  yml: "yaml",
  yaml: "yaml",
  ini: "ini",
  cfg: "ini",
  conf: "ini",
  toml: "ini",
  md: "markdown",
  markdown: "markdown",
  html: "html",
  htm: "html",
  css: "css",
  scss: "scss",
  less: "less",
  xml: "xml",
  sql: "sql",
  ps1: "powershell",
  lua: "lua",
  pl: "perl",
  r: "r",
  swift: "swift",
  scala: "scala",
  dart: "dart",
  graphql: "graphql",
  proto: "protobuf",
  dockerfile: "dockerfile",
};

const NAME_LANGUAGE: Record<string, string> = {
  dockerfile: "dockerfile",
  makefile: "shell",
  "cmakelists.txt": "shell",
};

export function languageForFile(name: string): string {
  const lower = name.toLowerCase();
  if (NAME_LANGUAGE[lower]) return NAME_LANGUAGE[lower];
  const ext = lower.includes(".") ? lower.slice(lower.lastIndexOf(".") + 1) : "";
  return EXT_LANGUAGE[ext] ?? "plaintext";
}
