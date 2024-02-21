@echo off

:: Check for admin privileges
net session >nul 2>&1
if %errorLevel% neq 0 (
    echo Please run the installation script as an administrator.
    pause
    exit
)

:: Get the current directory
set CURR_DIR=%~dp0

:: Associate your application with magnet links 
REG ADD HKCR\magnet /ve /d "URL:Magnet Protocol" /f
REG ADD HKCR\magnet /v "URL Protocol" /t REG_SZ /d "" /f
REG ADD HKCR\magnet\shell\open\command /ve /d "\"%CURR_DIR%magneto.exe\" "\""%%1"\""" /f

:: Associate your application with .torrent file
REG ADD HKCR\.torrent /ve /d "torrent_file" /f
REG ADD HKCR\torrent_file /ve /d "URL:Torrent File Protocol" /f
REG ADD HKCR\torrent_file /v "URL Protocol" /t REG_SZ /d "" /f
REG ADD HKCR\torrent_file\shell\open\command /ve /d "\"%CURR_DIR%magneto.exe\" "\""%%1"\""" /f

echo Installation completed!
pause