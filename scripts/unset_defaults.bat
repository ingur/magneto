@echo off

:: Check for admin privileges
net session >nul 2>&1
if %errorLevel% neq 0 (
    echo Please run the uninstallation script as an administrator.
    pause
    exit
)

:: Remove your application's association with magnet links
REG DELETE HKCR\magnet /f

:: Remove your application's association with .torrent files
REG DELETE HKCR\.torrent /f
REG DELETE HKCR\torrent_file /f

echo Uninstallation completed!
pause