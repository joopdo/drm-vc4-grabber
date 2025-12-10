#!/bin/bash

# Commit script for DRM resource management stability fix
# This adds to the existing PR: joopdo:feature/4k-detection-workaround

set -e

echo "Preparing commit for existing PR: feature/4k-detection-workaround..."

# Check if we're in a git repository
if [ ! -d ".git" ]; then
    echo "Error: Not in a git repository"
    exit 1
fi

# Check current branch
CURRENT_BRANCH=$(git branch --show-current)
echo "Current branch: $CURRENT_BRANCH"

# Switch to the PR branch if not already on it
if [ "$CURRENT_BRANCH" != "feature/4k-detection-workaround" ]; then
    echo "Switching to feature/4k-detection-workaround branch..."
    git checkout feature/4k-detection-workaround || {
        echo "Error: Could not switch to feature/4k-detection-workaround branch"
        echo "Make sure the branch exists or create it first"
        exit 1
    }
fi

# Add the key files that were modified for the stability fix
echo "Adding modified files..."

# Only add files that exist
FILES_TO_ADD=(
    "src/dump_image.rs"
    "src/main.rs"
    "deploy.sh"
)

# Add optional files if they exist
OPTIONAL_FILES=(
    "src/connection_manager.rs"
    "src/diagnostics.rs"
    "src/system_monitor.rs"
    ".kiro/specs/drm-vc4-grabber-stability/tasks.md"
)

for file in "${FILES_TO_ADD[@]}"; do
    if [ -f "$file" ]; then
        echo "Adding $file"
        git add "$file"
    else
        echo "Warning: $file not found, skipping"
    fi
done

for file in "${OPTIONAL_FILES[@]}"; do
    if [ -f "$file" ]; then
        echo "Adding optional file $file"
        git add "$file"
    else
        echo "Optional file $file not found, skipping"
    fi
done

# Check what we're about to commit
echo ""
echo "Files to be committed:"
git diff --cached --name-only

echo ""
echo "Commit summary:"
git diff --cached --stat

# Commit with a concise message
echo ""
echo "Committing changes..."
git commit -m "Fix DRM resource leaks causing Kodi crashes

- Fix GEM handle cleanup in dump_framebuffer_to_image()
- Clean up ALL GEM handles, not just handles[0]
- Add duplicate handle detection to avoid cleanup warnings
- Update deployment script for LibreELEC compatibility

Resolves critical stability issue where grabber would exhaust DRM 
resources and cause Kodi video playback failures. Kodi now remains 
stable during video playback with grabber running."

# Push to the PR branch
echo ""
echo "Pushing to feature/4k-detection-workaround branch..."
git push origin feature/4k-detection-workaround

echo ""
echo "✅ Successfully pushed to PR branch!"
echo ""
echo "Summary: Fixed critical DRM resource management issue"
echo "Result: Kodi now remains stable during video playback with grabber running"
echo "PR: joopdo:feature/4k-detection-workaround updated"