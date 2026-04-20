const HOST_NAME = "io.github.codegoddy.apexshot";

document.addEventListener('DOMContentLoaded', async () => {
  const statusEl = document.getElementById('status');
  const instructionsEl = document.getElementById('instructions');
  const testBtn = document.getElementById('test-btn');
  
  // Test native host connection
  try {
    console.log('[ApexShot] Pinging native host:', HOST_NAME);
    const response = await chrome.runtime.sendNativeMessage(HOST_NAME, {
      cmd: "ping"
    });
    console.log('[ApexShot] Ping response:', response);
    
    if (response && response.ok) {
      statusEl.className = 'status success';
      statusEl.textContent = '✓ Native host connected! You can now capture webpages.';
      instructionsEl.style.display = 'none';
    } else {
      throw new Error('Native host responded with error');
    }
  } catch (error) {
    console.error('[ApexShot] Ping failed:', error.message);
    // Try auto-registration
    statusEl.className = 'status info';
    statusEl.textContent = 'Setting up native host...';
    
    try {
      console.log('[ApexShot] Attempting auto-register with extension_id:', chrome.runtime.id);
      const registerResponse = await chrome.runtime.sendNativeMessage(HOST_NAME, {
        cmd: "auto_register",
        extension_id: chrome.runtime.id
      });
      console.log('[ApexShot] Auto-register response:', registerResponse);
      
      if (registerResponse && registerResponse.ok) {
        statusEl.className = 'status success';
        statusEl.textContent = '✓ Native host configured! You can now capture webpages.';
        instructionsEl.style.display = 'none';
      } else {
        throw new Error(registerResponse?.message || 'Auto-registration failed');
      }
    } catch (regError) {
      console.error('[ApexShot] Auto-register failed:', regError.message);
      statusEl.className = 'status error';
      statusEl.textContent = '✗ ' + (regError.message || 'Native host not found');
      instructionsEl.style.display = 'block';
      testBtn.style.display = 'block';
    }
  }
  
  testBtn.addEventListener('click', async () => {
    testBtn.disabled = true;
    testBtn.textContent = 'Testing...';
    
    try {
      const response = await chrome.runtime.sendNativeMessage(HOST_NAME, {
        cmd: "ping"
      });
      
      if (response && response.ok) {
        statusEl.className = 'status success';
        statusEl.textContent = '✓ Native host connected! You can now capture webpages.';
        instructionsEl.style.display = 'none';
        testBtn.style.display = 'none';
      } else {
        throw new Error('Native host responded with error');
      }
    } catch (error) {
      statusEl.className = 'status error';
      statusEl.textContent = '✗ ApexShot daemon not running. Please log out and log back in';
    } finally {
      testBtn.disabled = false;
      testBtn.textContent = 'Test Connection';
    }
  });
});
