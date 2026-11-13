# Response Style (always loaded)

How to answer in chat. `engineering-prose.md` governs commit bodies, PR descriptions and docs;
`anti-ai-slop.md` governs word choice in anything committed or posted.

Be concise. Default to the shortest complete answer. Answer first: no preamble, no restating the
question. No summaries or recaps at the end.

No hedging filler: "it's worth noting", "it's important to", "keep in mind".

**Ask with `AskUserQuestion`**, recommended option first, when a decision is the user's. A loose
end you noticed and are not doing ("X still needs Y") is a question too: ask it, or, if it is in
scope, cheap and reversible, do it and say so.

Plain prose over bullets and headers unless the content needs structure. Simple question: 1-3
sentences.

**When reporting a problem**: problem (one line), why (the mechanism), how it may affect (what a
learner, instructor, admin or the server sees), risks.

**When explaining how data moves, draw it inline**: a data flow diagram for client -> API ->
handler -> repo -> database, or a state diagram for an attempt or session. Never publish a
separate file for a diagram.

**Numbers carry units and a source.** "p95 autosave 62 ms (k6, 100 users, 1 vCPU)" not "fast".

**Say which "done" you mean** (`CLAUDE.md` -> Standing prohibitions).

## Status table

End every reply in which you did work with the status table, the operator's view of what is being
done and what is left.

| ID | Status | Work |
| --- | --- | --- |
| a4c | Doing | Four-case permission tests for `/api/v1/courses` |
| 1f0 | Pending | you: choose the slug format for organizations (spec 02) |
| 9e2 | Completed | Layering check covers build dependencies |

- **ID**: three lowercase hex characters, made up when the row first appears and kept all
  session; never reused. The operator refers to rows by it.
- Write it as plain markdown, not in a code fence. It ends the reply; only a `**Waiting on you:**`
  section (one `- ` line per thing the operator must do or answer) may follow it.
- **Doing**: this turn's work in progress. **Pending**: everything still to do, including work
  waiting on the operator (start it `you:`) or on another agent. **Completed**: finished since the
  last table, every step it names done.
- Every operator request and every task you generate gets a row the turn it appears and stays
  until completed. Never drop a row to look finished.
