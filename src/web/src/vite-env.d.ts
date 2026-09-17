/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_BITFORGE_VERSION?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
