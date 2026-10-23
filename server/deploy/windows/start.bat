@echo off
rem Start voice-llama and voice-gateway on the server machine over ssh.
call "%~dp0voice-config.bat"
echo Starting voice server on %REMOTE_HOST% ...
ssh %REMOTE_HOST% "systemctl --user start voice-llama voice-gateway && systemctl --user is-active voice-llama voice-gateway"
if errorlevel 1 (
    echo FAILED. Check: systemctl --user status voice-llama voice-gateway
) else (
    echo Started.
)
pause
