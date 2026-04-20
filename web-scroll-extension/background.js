const HOST_NAME = "io.github.codegoddy.apexshot";
const SCROLL_DELAY_MS = 700;
const CAPTURE_RETRY_DELAY_MS = 1000;
const MAX_CAPTURE_RETRIES = 3;
const OVERLAP_PX = 80;
const MAX_STEPS = 120;
const BOTTOM_RECHECK_LIMIT = 2;

async function testNativeHostConnection() {
  try {
    const response = await chrome.runtime.sendNativeMessage(HOST_NAME, {
      cmd: "ping"
    });
    return response && response.ok;
  } catch (error) {
    // Try auto-registration
    try {
      const registerResponse = await chrome.runtime.sendNativeMessage(HOST_NAME, {
        cmd: "auto_register",
        extension_id: chrome.runtime.id
      });
      return registerResponse && registerResponse.ok;
    } catch (regError) {
      return false;
    }
  }
}

chrome.action.onClicked.addListener(async (tab) => {
  if (!tab || !tab.id || typeof tab.windowId !== "number") {
    return;
  }

  if (!tab.url || !(tab.url.startsWith("http://") || tab.url.startsWith("https://"))) {
    chrome.notifications.create({
      type: "basic",
      iconUrl: "icon-128.png",
      title: "ApexShot Web Scroll Capture",
      message: "Only http/https pages are supported"
    });
    return;
  }

  // Test native host connection first (will try auto-registration)
  const isConnected = await testNativeHostConnection();
  if (!isConnected) {
    chrome.notifications.create({
      type: "basic",
      iconUrl: "icon-128.png",
      title: "ApexShot Not Connected",
      message: "Please log out and log back in to start the ApexShot daemon",
      priority: 2
    });
    return;
  }

  try {
    chrome.action.setIcon({
      path: {
        "16": "icon-16.png",
        "48": "icon-48.png",
        "128": "icon-128.png"
      },
      tabId: tab.id
    });
    chrome.action.setTitle({
      tabId: tab.id,
      title: "Capturing webpage..."
    });

    const metrics = await getPageMetrics(tab.id);
    const stitchedDataUrl = await captureAndStitch(tab, metrics);
    
    chrome.action.setTitle({
      tabId: tab.id,
      title: "Sending to ApexShot..."
    });

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

    chrome.notifications.create({
      type: "basic",
      iconUrl: "icon-128.png",
      title: "Capture Successful",
      message: "Screenshot sent to ApexShot for editing"
    });
  } catch (error) {
    console.error("ApexShot web scroll capture failed", error);
    chrome.notifications.create({
      type: "basic",
      iconUrl: "icon-128.png",
      title: "Capture Failed",
      message: error.message || "Failed to capture webpage",
      priority: 2
    });
  } finally {
    chrome.action.setTitle({
      tabId: tab.id,
      title: "Capture full webpage"
    });
  }
});

async function captureAndStitch(tab, metrics) {
  const dpr = Math.max(1, metrics.devicePixelRatio || 1);
  const viewportHeightCss = Math.max(1, metrics.viewportHeight);
  const viewportWidthCss = Math.max(1, metrics.captureViewportWidth || metrics.viewportWidth);
  const step = Math.max(1, viewportHeightCss - OVERLAP_PX);

  const slices = [];
  let targetY = 0;
  let lastCapturedY = null;
  let knownMaxScrollY = Math.max(0, metrics.totalHeight - viewportHeightCss);
  let dynamicTotalHeight = metrics.totalHeight;
  let bottomRechecks = 0;

  await preparePageForCapture(tab.id);
  try {
    for (let i = 0; i < MAX_STEPS; i += 1) {
      await setScrollY(tab.id, targetY);
      await sleep(SCROLL_DELAY_MS);

      const scrollState = await getScrollState(tab.id);
      const actualY = scrollState.scrollY;
      knownMaxScrollY = Math.max(knownMaxScrollY, scrollState.maxScrollY);
      dynamicTotalHeight = Math.max(dynamicTotalHeight, scrollState.totalHeight);

      await setFixedAndStickyVisibility(tab.id, i > 0);

      const dataUrl = await captureVisibleTabWithQuota(tab.windowId);
      slices.push({ yCss: actualY, dataUrl });

      const stuckAtSameY = lastCapturedY !== null && Math.abs(actualY - lastCapturedY) < 1;
      lastCapturedY = actualY;

      if (actualY + 1 >= knownMaxScrollY) {
        const refreshedState = await getScrollState(tab.id);
        knownMaxScrollY = Math.max(knownMaxScrollY, refreshedState.maxScrollY);
        dynamicTotalHeight = Math.max(dynamicTotalHeight, refreshedState.totalHeight);

        if (actualY + 1 >= knownMaxScrollY) {
          bottomRechecks += 1;
          if (stuckAtSameY || bottomRechecks >= BOTTOM_RECHECK_LIMIT) {
            break;
          }
        } else {
          bottomRechecks = 0;
        }
      } else {
        bottomRechecks = 0;
      }

      targetY = Math.min(actualY + step, knownMaxScrollY);
    }
  } finally {
    await restorePageAfterCapture(tab.id, metrics.initialScrollY);
  }

  if (slices.length === 0) {
    throw new Error("No slices captured");
  }

  const outputWidthPx = Math.round(viewportWidthCss * dpr);
  const outputHeightPx = Math.round(dynamicTotalHeight * dpr);

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

  let previousBottomCss = 0;
  for (let index = 0; index < slices.length; index += 1) {
    const slice = slices[index];
    const blob = await dataUrlToBlob(slice.dataUrl);
    const bitmap = await createImageBitmap(blob);
    const overlapCss = index === 0 ? 0 : Math.max(0, previousBottomCss - slice.yCss);
    const srcY = Math.min(bitmap.height, Math.round(overlapCss * dpr));
    const destY = Math.round((slice.yCss + overlapCss) * dpr);
    const drawHeight = Math.min(bitmap.height - srcY, outputHeightPx - destY);

    if (drawHeight > 0) {
      ctx.drawImage(
        bitmap,
        0,
        srcY,
        Math.min(bitmap.width, outputWidthPx),
        drawHeight,
        0,
        destY,
        outputWidthPx,
        drawHeight
      );
    }

    previousBottomCss = Math.max(previousBottomCss, slice.yCss + viewportHeightCss);
    bitmap.close();
  }

  const stitchedBlob = await canvas.convertToBlob({ type: "image/png" });
  const stitchedBase64 = await blobToBase64(stitchedBlob);
  return `data:image/png;base64,${stitchedBase64}`;
}

async function captureVisibleTabWithQuota(windowId) {
  let lastError = null;

  for (let attempt = 0; attempt < MAX_CAPTURE_RETRIES; attempt += 1) {
    try {
      return await chrome.tabs.captureVisibleTab(windowId, { format: "png" });
    } catch (error) {
      lastError = error;
      const message = error && error.message ? error.message : String(error);
      if (!message.includes("MAX_CAPTURE_VISIBLE_TAB_CALLS_PER_SECOND")) {
        throw error;
      }
      await sleep(CAPTURE_RETRY_DELAY_MS * (attempt + 1));
    }
  }

  throw lastError || new Error("captureVisibleTab failed");
}

async function preparePageForCapture(tabId) {
  await chrome.scripting.executeScript({
    target: { tabId },
    func: () => {
      const root = document.documentElement;
      const body = document.body;

      if (root) {
        root.dataset.apexshotPrevScrollBehavior = root.style.scrollBehavior || "";
        root.style.scrollBehavior = "auto";
      }
      if (body) {
        body.dataset.apexshotPrevScrollBehavior = body.style.scrollBehavior || "";
        body.style.scrollBehavior = "auto";
      }

      let style = document.getElementById("__apexshot_scroll_capture_style__");
      if (!style) {
        style = document.createElement("style");
        style.id = "__apexshot_scroll_capture_style__";
        document.documentElement.appendChild(style);
      }
      style.textContent = "*, *::before, *::after { animation: none !important; transition: none !important; }";
    }
  });
}

async function setFixedAndStickyVisibility(tabId, hidden) {
  await chrome.scripting.executeScript({
    target: { tabId },
    args: [hidden],
    func: (shouldHide) => {
      for (const element of document.querySelectorAll("*")) {
        const computed = window.getComputedStyle(element);
        if (computed.position !== "fixed" && computed.position !== "sticky") {
          continue;
        }
        const rect = element.getBoundingClientRect();
        if (rect.width <= 0 || rect.height <= 0 || rect.bottom <= 0 || rect.top >= window.innerHeight) {
          continue;
        }
        if (!element.hasAttribute("data-apexshot-prev-visibility")) {
          element.setAttribute("data-apexshot-prev-visibility", element.style.visibility || "");
        }
        element.style.visibility = shouldHide ? "hidden" : (element.getAttribute("data-apexshot-prev-visibility") || "");
      }
    }
  });
}

async function restorePageAfterCapture(tabId, initialScrollY) {
  await chrome.scripting.executeScript({
    target: { tabId },
    args: [initialScrollY],
    func: (scrollY) => {
      for (const element of document.querySelectorAll("[data-apexshot-prev-visibility]")) {
        element.style.visibility = element.getAttribute("data-apexshot-prev-visibility") || "";
        element.removeAttribute("data-apexshot-prev-visibility");
        element.removeAttribute("data-apexshot-scroll-hidden");
      }

      const style = document.getElementById("__apexshot_scroll_capture_style__");
      if (style) {
        style.remove();
      }

      const root = document.documentElement;
      const body = document.body;
      if (root) {
        root.style.scrollBehavior = root.dataset.apexshotPrevScrollBehavior || "";
        delete root.dataset.apexshotPrevScrollBehavior;
      }
      if (body) {
        body.style.scrollBehavior = body.dataset.apexshotPrevScrollBehavior || "";
        delete body.dataset.apexshotPrevScrollBehavior;
      }

      window.scrollTo(0, scrollY);
    }
  });
}

async function getPageMetrics(tabId) {
  const [{ result }] = await chrome.scripting.executeScript({
    target: { tabId },
    func: () => ({
      initialScrollY: window.scrollY,
      viewportHeight: window.innerHeight,
      viewportWidth: window.innerWidth,
      captureViewportWidth: (() => {
        const widths = [
          document.documentElement ? document.documentElement.clientWidth : 0,
          document.body ? document.body.clientWidth : 0,
          window.innerWidth
        ].filter((value) => value > 0);
        return widths.length > 0 ? Math.min(...widths) : window.innerWidth;
      })(),
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
  const [{ result }] = await chrome.scripting.executeScript({
    target: { tabId },
    args: [y],
    func: (targetY) => {
      window.scrollTo(0, targetY);
      return window.scrollY;
    }
  });

  if (typeof result !== "number") {
    throw new Error("Unable to read scrolled position");
  }

  return result;
}

async function getScrollState(tabId) {
  const [{ result }] = await chrome.scripting.executeScript({
    target: { tabId },
    func: () => {
      const viewportHeight = window.innerHeight;
      const totalHeight = Math.max(
        document.body ? document.body.scrollHeight : 0,
        document.documentElement ? document.documentElement.scrollHeight : 0,
        document.body ? document.body.offsetHeight : 0,
        document.documentElement ? document.documentElement.offsetHeight : 0,
        viewportHeight
      );
      const maxScrollY = Math.max(0, totalHeight - viewportHeight);
      return {
        scrollY: window.scrollY,
        totalHeight,
        maxScrollY
      };
    }
  });

  if (!result || typeof result.scrollY !== "number" || typeof result.totalHeight !== "number" || typeof result.maxScrollY !== "number") {
    throw new Error("Unable to read live scroll state");
  }

  return result;
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
