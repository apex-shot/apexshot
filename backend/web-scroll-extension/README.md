# ApexShot Web Scroll Capture

## Build and install ApexShot

```bash
cd /home/codegoddy/Desktop/apexshot/backend/backend
cargo build --release
sudo install -m 755 target/release/apexshot /usr/local/bin/apexshot
sudo install -m 755 target/release/apexshot-capture /usr/local/bin/apexshot-capture
```

## Install the browser native host

Find your extension ID in `chrome://extensions` or `chromium://extensions`, then run:

```bash
/usr/local/bin/apexshot native-host install --extension-id <your_extension_id>
```

Example:

```bash
/usr/local/bin/apexshot native-host install --extension-id haoihhmnbejmglcpobppcdkkedejjkin
```

## Load the extension

1. Open `chrome://extensions` or `chromium://extensions`
2. Enable **Developer mode**
3. Click **Load unpacked**
4. Select:

```text
/home/codegoddy/Desktop/apexshot/backend/backend/web-scroll-extension
```

## Start the ApexShot daemon

```bash
/usr/local/bin/apexshot daemon
```

Keep it running while using webpage scroll capture.

## Capture a webpage

1. Open any `http://` or `https://` page
2. Scroll capture works on webpages only
3. Click the **ApexShot Web Scroll Capture** extension button
4. The extension captures the page, stitches the image, and sends it to ApexShot
5. ApexShot opens the imported result in the normal preview/editor flow

## Notes

- Scroll capture is currently limited to webpages
- The extension does not work on browser internal pages such as `chrome://` pages
- On GNOME Wayland, the native area selector and webpage scroll capture use different paths; webpage scroll capture comes from the extension import flow
- If the import fails, make sure the daemon is running and the native host manifest was installed with the correct extension ID
