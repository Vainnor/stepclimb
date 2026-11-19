---
"@stepclimb/ui": minor
---

Let `ModuleBoard` edit modules as well as items: `moduleEditor` draws an Edit button on each
module row ("Edit 2 Ground control") that opens the caller's module editor in the same side sheet,
controlled through `editingModuleId` or uncontrolled through `defaultEditingModuleId`. A disabled
board draws no module Edit buttons and no module sheet.
