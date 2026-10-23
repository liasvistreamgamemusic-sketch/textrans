@echo off
rem Show the state of voice-llama and voice-gateway on the server machine over ssh.
call "%~dp0voice-config.bat"
ssh %REMOTE_HOST% "systemctl --user is-active voice-llama voice-gateway"
pause
