# GNOME Extension Submission Guide

## Prerequisites

1. **GNOME Account**: Create an account at https://extensions.gnome.org
2. **Screenshots**: Take 3-5 screenshots showing the extension in action (see screenshots/README.md)
3. **Extension Package**: The zip file `apexshot-gnome-integration.zip` is ready

## Submission Steps

### 1. Prepare Screenshots

Take the required screenshots as described in `screenshots/README.md`:
- Screenshot preview stacking (drag preview window)
- Recording mask with controls
- Recording controls, mask visibility, and mic/speaker runtime state
- Full recording session
- Extension enabled in GNOME Extensions app

### 2. Submit to extensions.gnome.org

1. Go to https://extensions.gnome.org
2. Log in with your GNOME account
3. Click "Upload Extension" in the top right
4. Fill in the required information:
   - **Extension UUID**: `apexshot-gnome-integration@apexshot.github.io`
   - **Name**: ApexShot GNOME Integration
   - **Description**: Enhances ApexShot with GNOME Shell integration for window stacking, recording masks, and runtime overlays
   - **Version**: 2
   - **Supported GNOME versions**: 45, 46, 47, 48, 49
   - **Website**: https://github.com/apex-shot/apexshot
   - **Download URL**: You'll upload the zip file directly

5. Upload the zip file: `apexshot-gnome-integration.zip`
6. Upload your screenshots (3-5 images)
7. Submit for review

### 3. Review Process

The GNOME extension review process typically includes:
- Code review for security and stability
- Testing on supported GNOME versions
- Verification that the extension follows GNOME Shell guidelines
- Review may take 1-2 weeks

### 4. Post-Approval

Once approved:
- Users can install directly from GNOME Software
- The extension will be discoverable in the extensions.gnome.org marketplace
- You'll be able to push updates by uploading new versions

## Alternative: GitHub Releases

If you prefer to skip the official review process, you can host the extension on GitHub Releases:

1. Create a new GitHub release: `gh release create gnome-extension-v2`
2. Upload the zip file as an asset
3. Users can install via:
   ```bash
   wget https://github.com/apex-shot/apexshot/releases/download/gnome-extension-v2/apexshot-gnome-integration.zip
   gnome-extensions install apexshot-gnome-integration.zip
   ```

## Integration with ApexShot Onboarding

To integrate the extension installation into the ApexShot onboarding flow, you can:

1. **Detect GNOME Shell**: Check if the user is running GNOME
2. **Check Extension Status**: Use `gnome-extensions list` to see if the extension is installed
3. **Install Extension**: Download and install the extension zip file
4. **Enable Extension**: Use `gnome-extensions enable apexshot-gnome-integration@apexshot.github.io`
5. **Restart Shell**: Prompt user to restart GNOME Shell (Alt+F2, type "r", Enter)

Example installation command in onboarding:
```bash
# Download and install extension
wget https://github.com/apex-shot/apexshot/releases/download/gnome-extension-v2/apexshot-gnome-integration.zip
gnome-extensions install apexshot-gnome-integration.zip
gnome-extensions enable apexshot-gnome-integration@apexshot.github.io
```

## Testing Before Submission

Test the extension locally:
```bash
# Install locally
mkdir -p ~/.local/share/gnome-shell/extensions/apexshot-gnome-integration@apexshot.github.io
cp -r . ~/.local/share/gnome-shell/extensions/apexshot-gnome-integration@apexshot.github.io/

# Enable
gnome-extensions enable apexshot-gnome-integration@apexshot.github.io

# Check logs
journalctl --user -f | grep apexshot
```

## Notes

- The extension UUID changed from `apexshot-preview-helper@apexshot.github.io` to `apexshot-gnome-integration@apexshot.github.io`
- Update any references in the ApexShot Rust code to use the new UUID
- The extension requires ApexShot to be installed and running
- The extension only works on GNOME Shell 45+
