---
"@stepclimb/ui": minor
---

Add `VideoUploader` and `VideoUploadCard`: a video upload through tus 1.0.0 in 16 MiB chunks, with
progress, Pause, Resume and Cancel. An upload stopped by a lost connection goes on when the browser
is back online; an unfinished upload is kept in local storage and continues when the same file is
chosen after a reload. Once complete, the video is read every 10 seconds until it is ready or
failed. Each problem code of the video upload routes and each failed video's code has its own
message. Requests go through the caller's `send`, so the package needs no API client.

`AlertDialog` takes an optional `cancelLabel` for its dismiss button, for a dialog where "Cancel"
would be ambiguous. VideoUploader uses it: once any bytes are sent, Cancel asks "Cancel this
upload?" with "Delete upload" and "Keep uploading".

Unfinished uploads are kept per owner (the caller's organization and user) and course, and only
addresses on the creation endpoint's origin under `/api/v1/video-uploads/` are listed; an explicit
sign-out clears an owner's with `clearResumeEntries`. Discard asks the same question as Cancel
when the server held any of the upload's bytes.
