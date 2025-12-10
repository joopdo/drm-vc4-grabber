#!/bin/bash

echo "Checking what changes we have vs the PR branch..."

# Check git status
echo "=== Git Status ==="
git status

echo ""
echo "=== Files that differ from origin/feature/4k-detection-workaround ==="
git diff origin/feature/4k-detection-workaround --name-only

echo ""
echo "=== Detailed diff for src files ==="
git diff origin/feature/4k-detection-workaround src/ || echo "No differences in src/"

echo ""
echo "=== Recent commits on this branch ==="
git log --oneline -5

echo ""
echo "=== Check if our DRM fix is actually in the current files ==="
echo "Checking for GEM handle cleanup in dump_image.rs:"
if grep -q "closed_handles" src/dump_image.rs 2>/dev/null; then
    echo "✅ DRM fix found in dump_image.rs"
else
    echo "❌ DRM fix NOT found in dump_image.rs"
fi

echo ""
echo "Checking for connection manager in main.rs:"
if grep -q "connection_manager" src/main.rs 2>/dev/null; then
    echo "✅ Connection manager found in main.rs"
else
    echo "❌ Connection manager NOT found in main.rs"
fi