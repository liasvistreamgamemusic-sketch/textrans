@echo off
rem Stop voice-gateway and voice-llama on the server machine over ssh.
call "%~dp0voice-config.bat"
echo Stopping voice server on %REMOTE_HOST% ...
ssh %REMOTE_HOST% "systemctl --user stop voice-gateway voice-llama"
if errorlevel 1 (
    echo FAILED to stop.
) else (
    echo Stopped.
)
pause
