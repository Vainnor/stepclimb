---
"@stepclimb/ui": minor
---

Add `ButtonLink`, React Aria's `Link` drawn with `Button`'s intents and sizes, for navigation that
reads as an action, such as a sign-in provider's button. It stays an anchor with its `href`, goes
through the app's `RouterProvider` with any `routerOptions`, and wraps a long label by growing in
height instead of overflowing. Like a button, it is not text-selectable and a long press on iOS
opens no link menu: `base.css` adds `[data-slot="button-link"]` to the rule that sets
`user-select: none` and `-webkit-touch-callout: none` on controls.
