# Session Wrap-up Workflow

At the end of each coding session, follow this standardized process to properly document and commit all work:

## 1. Create Detailed Commit Messages
Write comprehensive commit messages that clearly describe:
- **Problem**: What issue was being addressed
- **Solution**: How the issue was resolved
- **Files Modified**: List the key files that were changed
- **Impact**: What the fix accomplishes for users

Use conventional commit format:
```
fix: resolve shader compilation error in 9-Point Mesh Gradient

- Fixed extra closing parenthesis in rand_offset function
- Cleared build cache to ensure changes take effect
- Updated both main site and video generator shader files

Files modified:
- site/src/lib/video-renderer/background-effects/effects/gradients/NinePointMeshGradientShader.ts
- video-gen-and-proc/src/components/effects/ninePointMeshGradient/NinePointMeshGradientShader.ts

```

## 2. Create Development Log Entry
Create a new markdown file in `_docs/devlog/` with the filename format `YYYY-MM-DD-descriptive-slug.md`:
- **Filename**: Use current date and descriptive slug (e.g., `2025-10-01-tauri-project-setup.md`)
- **Content**: Follow the standard devlog entry format below

Create the file with this format:
```markdown
# YYYY-MM-DD - Descriptive Title

**Problem:** [Clear description of the issue]

**Root Cause:** [Technical explanation of why it happened]

**Solution:**
1. [Step 1 description]
2. [Step 2 description]
3. [Step 3 description]

**Files Modified:**
- `/path/to/file1.ts`
- `/path/to/file2.tsx`

**Outcome:** [Description of final result and user impact]
```

If `_docs/devlog/` directory doesn't exist, create it first.

## 3. Commit and Push Changes
Execute the complete git workflow:
```bash
# Add all changes
git add .

# Create commit with detailed message
git commit -m "[detailed commit message]"

# Push to remote repository
git push origin main
```
Make sure not to add the following to the commit message:
"🤖 Generated with Claude Code

Co-Authored-By: Claude noreply@anthropic.com"


## 4. Verification
Ensure all changes are properly committed by:
- Checking `git status` shows no uncommitted changes
- Verifying the commit appears in the remote repository
- Confirming the dev log entry file exists in `_docs/devlog/`

This workflow ensures comprehensive documentation of all development work and maintains a clear history of project evolution for future reference.