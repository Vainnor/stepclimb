# vZDC brief

The vZDC brief tab of the Stepclimb plan (final plan v1.0, 12 Nov 2026), as Markdown. Source: [the plan artifact](https://claude.ai/artifact/ABMCETKdv4QtXiWhdrJ8VH). Converted for the repository with American spelling (`docs/adr/0020-american-english.md`); the wording is otherwise the plan's. Changes to this file are the owner's (`.claude/rules/research-and-decisions.md`).

### What we're doing

Build **Stepclimb**, a general-purpose, open-source learning platform that any organization can use, from a company training its staff to a school or a VATSIM facility. They can host it themselves, or have it hosted for them. It's a complete LMS on its own, with public APIs so other systems can sync people, manage permissions and pull data. VATSIM support is one integration built on those APIs. vZDC's training is the first intended deployment, and vZDC can adopt it if the pilot goes well.

- **Pilot first, with two go/no-go checkpoints.** The VATUSA Moodle stays in use until the pilot passes.
- **Built-in training games** that no off-the-shelf LMS has: a flight plan editor game, CRC/ERAM command drills, matching and fill-in-the-blank. Every run has fresh problems, is graded instantly, and is recorded for trainers.
- **Works on phones from the pilot** as an installable web app with notifications. Desktop and iPhone apps follow at the 1.0 release.
- **Cost to vZDC:** about $5–15 a month for hosting from go-live, plus staff time to move course content. Development is the maintainer's own time, using AI coding agents.

## Why

- **The current system is slow and hard to use.** Students struggle to find their next step, and staff find building courses and exams painful.
- **It isn't connected to our training records,** so checking that theory is done before an OTS is manual.
- **We can't change it:** no branding, no VATSIM sign-in, no control over how exams work.
- **It doesn't work well on phones,** where many students study.
- **It can't do the hands-on practice we need.** No LMS we've looked at has flight plan or controller-command practice, so it would have to be custom-built either way.

## What training staff need

These are the requirements from the training department. Everything in the plan serves them.

| Requirement | What it means |
| --- | --- |
| Trainers can see progress at a glance | For each student: which lessons are done, time spent on each, every attempt, and exactly which answers were right and wrong. Filterable by rating and course. The website can link to it later if vZDC wants. |
| Easy access for students | VATSIM sign-in, no separate account. "Next up" is the first thing they see. Works on a phone. |
| Video in lessons | Videos inside lesson pages, with watch progress tracked. A lesson can require watching most of a video before it counts as done. |
| Quick drill games | Matching, fill in the blank and put-in-order games, built from question banks the TA controls. |
| Flight plan editor game | The student gets flight plans with mistakes (wrong altitude for direction, bad route, wrong equipment, invalid SID or STAR) and fixes them in a CRC/ERAM-style editor. |
| CRC/ERAM command games | The student gets a situation, such as "amend N123AB to FL240", and types the right command. |
| Unlimited, repeatable practice | New problems every time, generated from real navigation data and vZDC's own procedures, so students can't memorize one answer sheet. |
| Always correct | Answers are checked by rules on our server, not typed in by hand. Every problem is checked against those rules before a student sees it. Anyone can flag a problem that looks wrong. |
| Instant feedback, recorded | Self-graded. Each answer, its result and the time spent go to the trainer view. |
| Feedback is a setting | Staff choose how much students see, per quiz or game. Practice can explain every answer. Tests can show only right or wrong, or nothing until the end. Exams can hide results until staff release them. Trainers always see everything. |

## What vZDC gets, and what it costs

**Benefits**

- Course modules unlock in order and by rating. If vZDC later wants it, the website can listen for completions and update training records through Stepclimb's API and webhooks. Stepclimb doesn't depend on that.
- Exams with random questions on every attempt, map-click and phraseology fill-in questions, timed attempts, retake cooldowns and per-question statistics.
- Training games with unlimited practice and instant feedback: flight plan editing, runway headings (with a speed-and-accuracy sprint mode), clearances, readbacks, METAR to runway configuration, CRC/ERAM commands, daily review, matching and fill-in-the-blank.
- A trainer view showing completion, time spent, and every answer each student gave.
- VATSIM sign-in, open only to our home and visiting controllers.
- vZDC colors, logos and wording, and a link back to the website.
- Phone-friendly from the pilot. Desktop and iPhone apps with offline reading at release.
- Cheap to run on a small server.
- Open source, so other ARTCCs could use it too.

**Costs and downsides**

- About 4 months to the pilot and about 6 months to 1.0 (see the timeline). AI coding agents write most of the code. The pace is set by the maintainer's review and testing, the TA's rule writing, and the length of the pilot.
- Training staff must write down vZDC's rules (preferred routes, altitudes, LOAs) and the commands to drill, so the games can check answers.
- One developer, so progress depends on one person's time.
- New software: expect early bugs and fewer features than a mature LMS.
- We're responsible for security, updates and backups.
- Courses and questions are rebuilt by staff in Stepclimb. There's no automatic import from the VATUSA Moodle.

## Risks

| Risk | Likelihood | Mitigation |
| --- | --- | --- |
| Agent-written code hides bugs or security holes | Medium | Specs before code, strict CI gates, line-by-line human review of authentication, permissions, grading and billing, and a security review before the pilot. |
| The project runs late or stalls | Medium | Two checkpoints with pass criteria. vZDC keeps using the VATUSA Moodle until the pilot passes, so nothing depends on Stepclimb before then. |
| The main developer steps away | Medium | Public code under an open-source license that anyone can pick up, thorough documentation and design records, and vZDC holds its own server and data once adopted. |
| Member data is exposed | Low | Proven security libraries, automated checks, a security review before go-live, minimal personal data, nightly backups with tested restores. |
| A training game marks a correct answer wrong, or the reverse | Medium | Rules are reviewed by the TA before release. Every generated problem is checked before it's shown. A library of known cases is tested on every change. A "this looks wrong" button sends the exact problem to staff, and affected results can be regraded. |
| Students or staff don't like it | Low | A pilot with 10–20 people and a survey before wider rollout. Training staff design the courses. |
| Conflict of interest: a staff member's personal project becomes ARTCC infrastructure | Medium | The written terms below, agreed before work starts. |

## Terms

- **Open source.** vZDC can use, modify and host it forever, whatever happens to the developer.
- **vZDC owns its instance and data.** The server, the `learn.vzdc.org` domain, backups and admin accounts are held by vZDC staff.
- **No money changes hands.** vZDC pays hosting only. The apps are published under the maintainer's own Apple developer account, at no cost to vZDC.
- **Courses and exams can be exported** at any time in documented, standard formats.
- **Privacy:** member data stays in vZDC's instance, follows VATSIM data-protection rules, and is included in data-export and deletion requests.

## Timeline and checkpoints

Weeks count from the start of development, which doesn't wait on approval. AI coding agents build each milestone from its written spec, and the maintainer reviews and tests the work at about 20 hours a week. If review takes longer than expected, allow up to 1.5× these durations. The pilot length and Apple's app review don't speed up.

- **Weeks 0–2**: **Foundations.** Sign-in (including VATSIM), branding, roles, admin tools and a test server.
- **Week ~2**: **Checkpoint 1: on track?** Pass if staff can sign in with VATSIM on the test server and see the branded app, and the work is within about a week of plan. If not, re-plan the scope.
- **Weeks 2–8**: **Courses and exams.** Modules and pages, uploaded and linked video with watch tracking, due dates and reminders, recurring training, the exam engine, phone-friendly web app. The **iPhone app test** runs in week ~3: a few days confirming the Tauri approach, with Expo as the fallback.
- **Weeks 8–11**: **Training games.** The game engine, sprint mode, matching, fill-in-the-blank and ordering games, daily review, the flight plan editor and runway heading games, and the trainer view of time spent and every answer. The TA's routing, altitude and departure heading rules, with worked examples, are needed by **week 8**. This is the most likely thing to hold up the pilot.
- **Weeks 11–14**: **Integrations and pilot readiness.** User sync with managers, external permission management, reporting and export APIs, webhooks, the VATSIM and VATUSA connectors, backups, monitoring, documentation, and a security review of authentication, permissions and grading.
- **Weeks 14–18**: **Checkpoint 2: pilot.** One real course (for example S1 theory, including the flight plan and runway heading games) runs with 10–20 students for one training cycle (about 4 weeks), alongside the current system. Pass if completions, results and game answers are correct and reach trainers, there are no data or security problems, and at least 4 in 5 pilot students and instructors prefer it. If not, fix and re-pilot. vZDC stays on the VATUSA Moodle until it passes.
- **Weeks 14–24**: **Rollout and 1.0.** Built during and after the pilot: the CRC/ERAM command games, ATC scenarios, clearance builder, readback catcher and METAR game, and the desktop and iPhone apps. After the pilot passes, move the remaining courses and stop using the VATUSA Moodle.

## What we need from leadership (for adoption)

Development goes ahead now as an independent project. These are needed only for vZDC to run the pilot and adopt it.

1. **Approval** of the pilot on the terms above.
2. **A content owner** appointed by the TA to move and rebuild theory courses and choose the pilot course. They'll also write down the rules and commands the training games check against: preferred routes, altitude rules, LOA restrictions, and the CRC/ERAM commands to drill.
3. **An owner for vZDC's instance** (suggested: WM). During development and the pilot it runs on the maintainer's homelab. After adoption it moves to vZDC's own servers, held by vZDC staff.
4. **A hosting budget** of about $5–15 a month once it moves to vZDC's servers.
5. **Confirmation from VATUSA** that facility theory training may live outside the VATUSA Academy. Rating exams stay with VATUSA.
