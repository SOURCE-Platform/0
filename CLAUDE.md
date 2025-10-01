# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

"0" is a cross-platform screen recording and activity tracking application with a privacy-first, local-first design. The application uses Tauri 2.x to bridge a Rust backend with a React TypeScript frontend.

## Development Commands

### Running the Application
```bash
# Start both Vite dev server (port 1420) and Rust backend with hot reload
npm run tauri dev

# Note: Closing the app window stops both servers
```

### Building
```bash
# Build production bundles for all platforms
npm run tauri build

# TypeScript compilation and Vite build (frontend only)
npm run build
```

### Frontend Development
```bash
# Run Vite dev server only (without Tauri)
npm run dev

# Preview production build
npm run preview
```

## Architecture

### Dual-Process Model
Tauri runs as two separate processes that communicate via IPC:
- **Rust Backend** (`src-tauri/`): Native system access, core logic, data processing
- **React Frontend** (`src/`): UI rendered in WebView, communicates with backend via Tauri commands

### Backend Structure (`src-tauri/src/`)
The Rust codebase is organized into three main modules:
- **`platform/`**: Platform-specific implementations (macOS, Windows, Linux)
- **`core/`**: Cross-platform business logic and shared functionality
- **`models/`**: Data structures, serialization, and type definitions

### Entry Points
- **Rust**: `main.rs` → `lib.rs::run()` → Tauri app initialization
- **Frontend**: `index.html` → `src/main.tsx`

### Communication Pattern
Frontend calls Rust via Tauri commands defined with `#[tauri::command]` macro in `lib.rs`. Commands are registered in the `.invoke_handler()` builder. Example:

```rust
// Backend (lib.rs)
#[tauri::command]
fn greet(name: &str) -> String { ... }

// Frontend (TypeScript)
import { invoke } from '@tauri-apps/api/core';
const result = await invoke('greet', { name: 'World' });
```

## Key Dependencies

### Backend (Cargo.toml)
- **tokio**: Async runtime for concurrent operations (full features enabled)
- **serde/serde_json**: Serialization for IPC and data storage
- **uuid**: Unique identifiers for recorded events
- **chrono**: Timestamp management for activity tracking

### Frontend (package.json)
- **React 19**: UI framework
- **Vite 7**: Build tool and dev server (fixed port 1420)
- **@tauri-apps/api**: Frontend bindings for Rust commands

## Development Workflow

### Hot Reload Behavior
- **Frontend changes**: Instant HMR via Vite
- **Rust changes**: App automatically restarts after recompilation (takes a few seconds)

### First Build
The initial `npm run tauri dev` downloads and compiles ~500 Rust dependencies, taking 5-10 minutes. Subsequent builds are incremental and much faster.

### Configuration Files
- **`tauri.conf.json`**: App metadata, window settings, build configuration
  - Frontend dev server: `http://localhost:1420` (strict port)
  - Production bundle output: `dist/`
- **`vite.config.ts`**: Frontend build settings, ignores `src-tauri/` from watch
- **`Cargo.toml`**: Rust dependencies and library configuration
  - Library name: `zero_lib` (suffixed to avoid Windows naming conflicts)

## Platform Support
Targets macOS, Windows, and Linux. Platform-specific code should be isolated in `src-tauri/src/platform/` using Rust's `#[cfg(target_os = "...")]` attributes.
