@echo off
rem Doble clic para bajar los modelos que le faltan a la app.
rem El -ExecutionPolicy Bypass es para que Windows no bloquee el script.
title Bajando los modelos del Transcriptor
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0descargar-modelos.ps1"
if errorlevel 1 (
  echo.
  echo Algo salio mal. Podes volver a ejecutar esto: lo ya bajado no se repite.
  pause
)
