# ApexShot Web Scroll Capture

## Chrome Web Store Installation

### For Users (Deb Package)

1. Install ApexShot from the deb package:

```bash
sudo apt install apexshot
```

2. Install the [ApexShot Web Scroll Capture extension](https://chrome.google.com/webstore) from the Chrome Web Store
3. Navigate to any webpage and click the extension button to capture

**That's it!** The daemon auto-starts on login, and the extension will automatically configure itself on first use. No manual setup needed.

### For Developers

#### Load the extension (for development)

1. Install ApexShot from deb package first: `sudo apt install apexshot`
2. Open `chrome://extensions` or `chromium://extensions`
3. Enable **Developer mode**
4. Click **Load unpacked**
5. Select:

```text
/home/codegoddy/Desktop/apexshot/web-scroll-extension
```

The daemon auto-starts on login, so no manual startup needed.

## Capture a webpage

1. Open any `http://` or `https://` page
2. Scroll capture works on webpages only
3. Click the **ApexShot Web Scroll Capture** extension button
4. The extension captures the page, stitches the image, and sends it to ApexShot
5. ApexShot opens the imported result in the normal preview/editor flow

## Features

- **Full webpage scrolling**: Captures entire webpages by automatically scrolling and stitching screenshots
- **Automatic import**: Captured screenshots are immediately sent to ApexShot for editing
- **Auto-configuration**: Extension automatically sets up native host on first use - no manual steps
- **Visual feedback**: Notifications show capture status and errors
- **Connection checking**: Extension verifies native host connection before capturing

## Notes

- Scroll capture is currently limited to webpages
- The extension does not work on browser internal pages such as `chrome://` pages
- On GNOME Wayland, the native area selector and webpage scroll capture use different paths; webpage scroll capture comes from the extension import flow
- The extension automatically configures the native host on first use - no manual setup required
- The extension requires the ApexShot desktop app to be installed and running

## Chrome Web Store Submission

To submit this extension to the Chrome Web Store:

1. Package the extension files:
   - manifest.json
   - background.js
   - popup.html
   - popup.js
   - icon-16.png
   - icon-48.png
   - icon-128.png

2. Create store listing assets:
   - Screenshots (1280x800 or 640x400)
   - Promotional images (optional)
   - Detailed description
   - Privacy policy URL

3. Upload to Chrome Web Store Developer Dashboard

4. Set pricing (free) and distribution (public)

5. Submit for review
