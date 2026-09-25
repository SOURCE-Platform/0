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
- **ffmpeg-sys-next 8.0**: Low-level FFmpeg bindings for video encoding

### Frontend (package.json)
- **React 19**: UI framework
- **Vite 7**: Build tool and dev server (fixed port 1420)
- **@tauri-apps/api**: Frontend bindings for Rust commands

## System Requirements

### FFmpeg (Required for Video Encoding)

The application requires FFmpeg 8.0+ to be installed on the system for video encoding functionality.

**macOS (Homebrew):**
```bash
brew install ffmpeg
```

**Linux (Ubuntu/Debian):**
```bash
sudo apt install ffmpeg
```

**Windows (Chocolatey):**
```bash
choco install ffmpeg
```

**Verification:**
```bash
ffmpeg -version  # Should show version 8.0 or higher
```

The build system automatically detects FFmpeg installation on macOS. If FFmpeg is not found, video encoding will fail at runtime.

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

## Codebase Modularity Rules

- Keep every source file at or under 350 lines. This is a hard repo rule.
- If a change would push a file past that limit, split the implementation into smaller modules first.
- Do not leave touched oversized files in place. Refactor them during the same task unless there is a concrete blocker.
- Prefer module directories with focused files such as `types`, `service`, `queries`, `capture`, `indexing`, `hooks`, or `sections`.
- Keep entry-point files thin. `lib.rs`, large Tauri command files, and major React screens should compose smaller modules instead of holding full implementations.
- Split by responsibility, not by arbitrary chunks. Each file should have one clear job and stay readable in one screenful.
- Treat monolithic files as maintenance bugs. If you encounter one, plan the refactor before continuing feature work.
- Use the repo file-length audit during refactors so violations are visible in one command rather than by manual inspection.

## Credential-Vault Review Workflow

Substantial credential-vault work (spec revisions, design documents,
implementation milestones) follows a bounded review loop using the project
subagents in `.claude/agents/`:

1. The lead produces the candidate.
2. The candidate is frozen (no edits while it is under review).
3. Independent reviewers inspect it in fresh contexts:
   `security-reviewer` and `spec-reviewer` for spec/design work;
   `verification-reviewer` (plus `security-reviewer` where relevant) after
   implementation milestones.
4. The lead verifies every finding against the repository rather than
   accepting it blindly, and records a disposition for each.
5. Accepted blockers and important findings are fixed.
6. One bounded re-review of the fixes runs.
7. If no security-critical or spec-blocking problem remains, the checkpoint
   is closed. There are no unlimited review loops.

Stop and ask the owner if reviewers disagree materially about cryptography
or security, or if a proposed fix would change an owner-approved invariant.
Optional hardening and alternative architectures are not reasons to reopen
an approved design.
