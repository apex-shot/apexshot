const HOST_NAME = "io.github.codegoddy.cleanshitx";
const SCROLL_DELAY_MS = 350;
const OVERLAP_PX = 80;
const MAX_STEPS = 120;

chrome.action.onClicked.addListener(async (tab) => {
  if (!tab || !tab.id || typeof tab.windowId !== "number") {
    return;
  }

  if (!tab.url || !(tab.url.startsWith("http://") || tab.url.startsWith("https://"))) {
    console.error("CleanShotX web scroll capture only supports http/https pages");
    return;
  }

  try {
    const metrics = await getPageMetrics(tab.id);
    const stitchedDataUrl = await captureAndStitch(tab, metrics);
    const response = await chrome.runtime.sendNativeMessage(HOST_NAME, {
      cmd: "capture_web_scroll",
      png_data_url: stitchedDataUrl,
      page_url: tab.url || "",
      page_title: tab.title || ""
    });

    if (!response || !response.ok) {
      const msg = response && response.message ? response.message : "Native host import failed";
      throw new Error(msg);
    }
  } catch (error) {
    console.error("CleanShotX web scroll capture failed", error);
  }
});

async function captureAndStitch(tab, metrics) {
  const dpr = Math.max(1, metrics.devicePixelRatio || 1);
  const viewportHeightCss = Math.max(1, metrics.viewportHeight);
  const viewportWidthCss = Math.max(1, metrics.viewportWidth);
  const maxScrollY = Math.max(0, metrics.totalHeight - viewportHeightCss);
  const step = Math.max(1, viewportHeightCss - OVERLAP_PX);

  const slices = [];
  let targetY = 0;

  for (let i = 0; i < MAX_STEPS; i += 1) {
    await setScrollY(tab.id, targetY);
    await sleep(SCROLL_DELAY_MS);

    const dataUrl = await chrome.tabs.captureVisibleTab(tab.windowId, { format: "png" });
    slices.push({ yCss: targetY, dataUrl });

    if (targetY >= maxScrollY) {
      break;
    }
    targetY = Math.min(targetY + step, maxScrollY);
  }

  await setScrollY(tab.id, metrics.initialScrollY);

  if (slices.length === 0) {
    throw new Error("No slices captured");
  }

  const outputWidthPx = Math.round(viewportWidthCss * dpr);
  const outputHeightPx = Math.round(metrics.totalHeight * dpr);

  if (outputWidthPx <= 0 || outputHeightPx <= 0) {
    throw new Error("Invalid output dimensions");
  }

  if (outputWidthPx > 16384 || outputHeightPx > 65535) {
    throw new Error("Page too large to stitch in extension canvas");
  }

  const canvas = new OffscreenCanvas(outputWidthPx, outputHeightPx);
  const ctx = canvas.getContext("2d", { alpha: false });
  if (!ctx) {
    throw new Error("Failed to initialize canvas context");
  }

  for (const slice of slices) {
    const blob = await dataUrlToBlob(slice.dataUrl);
    const bitmap = await createImageBitmap(blob);
    const drawY = Math.round(slice.yCss * dpr);
    ctx.drawImage(bitmap, 0, drawY, outputWidthPx, bitmap.height);
    bitmap.close();
  }

  const stitchedBlob = await canvas.convertToBlob({ type: "image/png" });
  const stitchedBase64 = await blobToBase64(stitchedBlob);
  return `data:image/png;base64,${stitchedBase64}`;
}

async function getPageMetrics(tabId) {
  const [{ result }] = await chrome.scripting.executeScript({
    target: { tabId },
    func: () => ({
      initialScrollY: window.scrollY,
      viewportHeight: window.innerHeight,
      viewportWidth: window.innerWidth,
      totalHeight: Math.max(
        document.body ? document.body.scrollHeight : 0,
        document.documentElement ? document.documentElement.scrollHeight : 0,
        document.body ? document.body.offsetHeight : 0,
        document.documentElement ? document.documentElement.offsetHeight : 0,
        window.innerHeight
      ),
      devicePixelRatio: window.devicePixelRatio || 1
    })
  });

  if (!result) {
    throw new Error("Unable to read page metrics");
  }

  return result;
}

async function setScrollY(tabId, y) {
  await chrome.scripting.executeScript({
    target: { tabId },
    args: [y],
    func: (targetY) => {
      window.scrollTo(0, targetY);
    }
  });
}

async function dataUrlToBlob(dataUrl) {
  const response = await fetch(dataUrl);
  return response.blob();
}

async function blobToBase64(blob) {
  const buffer = await blob.arrayBuffer();
  let binary = "";
  const bytes = new Uint8Array(buffer);
  const chunkSize = 0x8000;
  for (let i = 0; i < bytes.length; i += chunkSize) {
    const chunk = bytes.subarray(i, i + chunkSize);
    binary += String.fromCharCode(...chunk);
  }
  return btoa(binary);
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
