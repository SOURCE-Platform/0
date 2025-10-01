# 0 - Screen Recording & Activity Tracking

Cross-platform screen recording and activity tracking application built with Tauri 2.x, Rust, and React TypeScript.

## Features

- Local-first data storage with privacy-first design
- Screen activity recording
- OS event tracking
- User input monitoring
- Cross-platform support (macOS, Windows, Linux)

## Tech Stack

- **Backend**: Rust with Tauri 2.x
- **Frontend**: React with TypeScript
- **Build Tool**: Vite
- **Runtime**: Tokio async runtime

## Development

```bash
# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

## Project Structure

```
src-tauri/
  src/
    platform/    # Platform-specific implementations
    core/        # Core business logic
    models/      # Data models and structures
```

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
