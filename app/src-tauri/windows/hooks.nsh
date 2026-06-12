; Default Apps registration. The bundler's own registration (the magnet
; class from the deep-link plugin and the .torrent file association) makes
; the schemes launchable, but Windows pickers (Settings "Default apps", the
; "How do you want to open this?" dialog) only list applications registered
; through RegisteredApplications + Capabilities, and only a choice made in
; those pickers is recorded as the hash-protected per-user default that
; survives another torrent client re-claiming the plain class keys.

!macro NSIS_HOOK_PREINSTALL
!macroend

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr SHCTX "Software\Classes\Magneto.magnet" "" "Magnet link"
  WriteRegStr SHCTX "Software\Classes\Magneto.magnet\DefaultIcon" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
  WriteRegStr SHCTX "Software\Classes\Magneto.magnet\shell\open\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""

  WriteRegStr SHCTX "Software\Classes\Magneto.torrent" "" "BitTorrent metadata file"
  WriteRegStr SHCTX "Software\Classes\Magneto.torrent\DefaultIcon" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
  WriteRegStr SHCTX "Software\Classes\Magneto.torrent\shell\open\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""

  WriteRegStr SHCTX "Software\Magneto\Capabilities" "ApplicationName" "Magneto"
  WriteRegStr SHCTX "Software\Magneto\Capabilities" "ApplicationDescription" "A media-focused torrent client"
  WriteRegStr SHCTX "Software\Magneto\Capabilities\URLAssociations" "magnet" "Magneto.magnet"
  WriteRegStr SHCTX "Software\Magneto\Capabilities\FileAssociations" ".torrent" "Magneto.torrent"
  WriteRegStr SHCTX "Software\RegisteredApplications" "Magneto" "Software\Magneto\Capabilities"

  ; Explorer and Settings cache associations; without this nudge the new
  ; registration may not show up until the next logon.
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue SHCTX "Software\RegisteredApplications" "Magneto"
  DeleteRegKey SHCTX "Software\Magneto\Capabilities"
  DeleteRegKey /ifempty SHCTX "Software\Magneto"
  DeleteRegKey SHCTX "Software\Classes\Magneto.magnet"
  DeleteRegKey SHCTX "Software\Classes\Magneto.torrent"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend
