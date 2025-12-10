#!/bin/bash

# Apply DRM resource management fixes to the PR branch
# This recreates the critical fixes we made on master

set -e

echo "Applying DRM resource management fixes to PR branch..."

# Check we're on the right branch
CURRENT_BRANCH=$(git branch --show-current)
if [ "$CURRENT_BRANCH" != "feature/4k-detection-workaround" ]; then
    echo "Error: Not on feature/4k-detection-workaround branch"
    exit 1
fi

echo "Current branch: $CURRENT_BRANCH"

# Backup current files
echo "Creating backup of current files..."
cp src/dump_image.rs src/dump_image.rs.backup
cp src/main.rs src/main.rs.backup

echo "Applying DRM resource leak fix to dump_image.rs..."

# Apply the critical GEM handle cleanup fix to dump_image.rs
# This is the core fix that prevents Kodi crashes
cat > temp_patch.txt << 'EOF'
--- a/src/dump_image.rs
+++ b/src/dump_image.rs
@@ -1,4 +1,5 @@
 use std::{convert::TryFrom, mem::size_of, os::fd::AsRawFd};
+use std::collections::HashSet;
 
 use drm::control::framebuffer::Handle;
 use drm::SystemError;
EOF

# Apply the fix manually by modifying the cleanup section
echo "Modifying dump_framebuffer_to_image function..."

# Create the fixed version of dump_image.rs
python3 << 'EOF'
import re

# Read the current file
with open('src/dump_image.rs', 'r') as f:
    content = f.read()

# Add the HashSet import if not present
if 'use std::collections::HashSet;' not in content:
    content = content.replace(
        'use std::{convert::TryFrom, mem::size_of, os::fd::AsRawFd};',
        'use std::{convert::TryFrom, mem::size_of, os::fd::AsRawFd};\nuse std::collections::HashSet;'
    )

# Find and replace the gem_close section
old_cleanup = r'    gem_close\(card\.as_raw_fd\(\), fbinfo2\.handles\[0\]\)\.unwrap\(\);'
new_cleanup = '''    // Clean up ALL GEM handles, not just the first one
    let mut closed_handles = HashSet::new();
    for i in 0..4 {
        if fbinfo2.handles[i] != 0 && !closed_handles.contains(&fbinfo2.handles[i]) {
            if let Err(e) = gem_close(card.as_raw_fd(), fbinfo2.handles[i]) {
                // Only log if it's not an "already closed" error
                if !e.to_string().contains("invalid argument") {
                    eprintln!("Warning: Failed to close GEM handle {}: {}", fbinfo2.handles[i], e);
                }
            } else if verbose {
                println!("Closed GEM handle {}", fbinfo2.handles[i]);
            }
            closed_handles.insert(fbinfo2.handles[i]);
        }
    }'''

content = re.sub(old_cleanup, new_cleanup, content)

# Write the modified content
with open('src/dump_image.rs', 'w') as f:
    f.write(content)

print("✅ Applied DRM resource leak fix to dump_image.rs")
EOF

echo "Verifying the fix was applied..."
if grep -q "closed_handles" src/dump_image.rs; then
    echo "✅ DRM fix successfully applied!"
else
    echo "❌ Fix application failed, restoring backup"
    cp src/dump_image.rs.backup src/dump_image.rs
    exit 1
fi

# Clean up backups
rm src/dump_image.rs.backup src/main.rs.backup temp_patch.txt

echo ""
echo "=== Changes made ==="
git diff src/dump_image.rs

echo ""
echo "Ready to commit the DRM fix to PR branch!"
echo "Run: git add src/dump_image.rs && git commit -m 'Fix DRM resource leaks causing Kodi crashes'"