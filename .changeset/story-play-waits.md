---
"@stepclimb/ui": patch
---

Stories whose play step raced the render now wait for it. Tabs' and ModuleBoard's Focus Visible
find their first tab or row before pressing Tab, so focus lands on it; the Menu, Editor,
VideoUploader, Tooltip, IconButton and SideNav stories that open an overlay wait for it to become
visible before checking it.
