---
"@stepclimb/ui": patch
---

`Button` and `IconButton` take back React Aria's pending announcement when they unmount. React
Aria announces a focused button turning pending, or back, with a hidden `role="img"` node labelled
by the button's id and keeps it 7 s; a button removed in that time, such as a sign-in step's
submit button when the step moves on, left the node pointing at an id no element has, which axe
reports as `role-img-alt`. The node now leaves the page with the button, including the page's
first announcement, which React Aria adds 100 ms late.
