---
"@stepclimb/ui": minor
---

Add the page editor as `@stepclimb/ui/editor`, a subpath of its own so Tiptap loads only where
it is imported. `Editor` holds only the nodes and marks of the page document model (spec 07 B8),
with a toolbar, a slash menu (`/` at the start of an empty paragraph), a block toolbar for the
settings of the block holding the caret, and panels that ask for an image's file and text
alternative and a video's address and title before inserting them. It gives the body in the
server's stored form (`toPageDoc`, every attribute written out, Tiptap's own attributes left out),
checks links the way the server does (`linkTarget`), takes control characters out of pasted text,
and shows a refusal from the server above the page and at the block its pointer names. Controlled
with `value` and `onChange`, uncontrolled with `defaultValue`.

`@stepclimb/ui` now declares `"sideEffects": ["*.css"]`: none of its modules does anything when
imported, so a bundler leaves out, or moves to a lazy chunk, the components an app's main bundle
does not use, even when they are reached through the package's index.
