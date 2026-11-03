#!/usr/bin/env python3
"""Set Finder's installer layout without controlling Finder or user windows."""
from pathlib import Path
import sys

from ds_store import DSStore
from mac_alias import Alias

volume = Path(sys.argv[1]).resolve()
background = volume / ".background/installer.png"
# The alias must reference the mounted volume, never a local staging path.
if not str(volume).startswith("/Volumes/"):
    raise SystemExit("Mount the installer under /Volumes before creating its layout")
settings = {
    "viewOptionsVersion":1, "backgroundType":2,
    "backgroundImageAlias":Alias.for_file(str(background)).to_bytes(),
    "backgroundColorRed":0.9569, "backgroundColorGreen":0.9647, "backgroundColorBlue":0.9725,
    "gridOffsetX":0.0, "gridOffsetY":0.0, "gridSpacing":100.0,
    "scrollPositionX":0.0, "scrollPositionY":0.0,
    "arrangeBy":"none", "showIconPreview":True, "showItemInfo":False,
    "labelOnBottom":True, "textSize":13.0, "iconSize":104.0,
}
with DSStore.open(str(volume / ".DS_Store"), "w+") as store:
    store["."]["vSrn"] = ("long",1)
    store["."]["icvl"] = ("type",b"icnv")
    store["."]["bwsp"] = {
        "WindowBounds":"{{180, 100}, {720, 404}}", "ShowToolbar":False,
        "ShowStatusBar":False, "ShowSidebar":False, "ContainerShowSidebar":False,
        "SidebarWidth":180,
        "ShowPathbar":False, "ShowTabView":False, "PreviewPaneVisibility":False,
    }
    store["."]["icvp"] = settings
    for name, point in {"MomentStays.app":(180,155),"Applications":(540,155)}.items():
        store[name]["Iloc"] = point
print("Configured Finder window, background and two installer icons")
