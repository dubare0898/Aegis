# Aegis operator console

React + TypeScript + Vite UI for the OITL loop: air picture, recommendations, Operator Y/N / Auto engage, scenario class picker, defeat log.

## Develop

With `aegis_api` running on `:8080`:

```bash
npm install
npm run dev
```

## Build for desktop / static serve

```bash
npm run build
```

`./scripts/launch-desktop.sh` rebuilds `dist` when `src` is newer. Docker/`aegis_api --console-dist` serve the built `dist`.
