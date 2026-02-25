import subprocess
import time
import psutil

CHROME_PATH = r"C:\Program Files\Google\Chrome\Application\chrome.exe"

def kill_chrome():
    for proc in psutil.process_iter(['name']):
        if proc.info['name'] and "chrome" in proc.info['name'].lower():
            proc.kill()

print("Closing existing Chrome...")
kill_chrome()
time.sleep(2)

print("Launching Chrome with UMS URL...")
subprocess.Popen([
    CHROME_PATH,
    "https://ums.lpu.in"
])

time.sleep(5)

print("Opening Google Search for LPU UMS...")
subprocess.Popen([
    CHROME_PATH,
    "https://google.com/search?q=lpu+ums"
])

print("Automation basic test completed.")
