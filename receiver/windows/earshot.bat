@echo off
rem Double-click this to run Earshot as your microphone.
rem
rem It needs a virtual audio cable, because Windows will not let a program invent a
rem microphone by itself. If none is installed, Earshot explains that and offers to
rem open the (free) VB-Cable download page for you.
rem
rem Once it is running: pick "CABLE Output" as your microphone in Discord, and type
rem the nine-digit pairing code it prints into the Earshot app on your phone.
title Earshot
"%~dp0earshot-receiver.exe" --virtual-mic
