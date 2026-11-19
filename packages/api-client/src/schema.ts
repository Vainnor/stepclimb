// SPDX-License-Identifier: Apache-2.0
// Generated from openapi/stepclimb.json by scripts/generate.mjs. Do not edit; run
// `just openapi` instead.
export interface paths {
    "/.well-known/stepclimb": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * The discovery document of the request's organization: its name, the sign-in methods it
         *     offers and its theme.
         */
        get: operations["getDiscovery"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/accounts": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * List an account's children, or without `parent_id` the topmost accounts the caller can see:
         *     the root for a holder of `account.view` at organization scope, otherwise each account where
         *     the caller holds it below no other such account. Sorted by name, then id.
         */
        get: operations["listAccounts"];
        put?: never;
        /** Create an account under `parent_id`. */
        post: operations["createAccount"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/accounts/{account_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read one account. */
        get: operations["getAccount"];
        put?: never;
        post?: never;
        /**
         * Delete an account that has no child accounts, no role assignments scoped to it and no
         *     courses.
         */
        delete: operations["deleteAccount"];
        options?: never;
        head?: never;
        /** Rename an account. A patch without `name` changes nothing. */
        patch: operations["updateAccount"];
        trace?: never;
    };
    "/api/v1/accounts/{account_id}/move": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Move an account and its subtree under `parent_id`. */
        post: operations["moveAccount"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/admission": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read the organization's admission switches and rules. */
        get: operations["getAdmissionSettings"];
        /**
         * Change the admission switches and rule lists the body names; send only what you change. A
         *     member the body leaves out keeps its value, and a body naming only current values changes
         *     nothing.
         */
        put: operations["updateAdmissionSettings"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/admission/webhook": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read the organization's admission webhook. The signing secret is never returned. */
        get: operations["getAdmissionWebhook"];
        /**
         * Create the organization's admission webhook, or change the members the body names. The
         *     answer of the call that creates it carries the signing secret, once.
         */
        put: operations["updateAdmissionWebhook"];
        post?: never;
        /** Delete the organization's admission webhook; sign-ins no longer call it. */
        delete: operations["deleteAdmissionWebhook"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/admission/webhook/rotate-secret": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Replace the webhook's signing secret with a new one, returned once. The old secret stops
         *     signing at once.
         */
        post: operations["rotateAdmissionWebhookSecret"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/branding": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read the organization's branding. */
        get: operations["getBranding"];
        /**
         * Replace the organization's branding. The server generates the light, dark and
         *     high-contrast themes from the inputs and saves them only when every contrast pair passes;
         *     `/theme.css` serves the new stylesheet from then on.
         */
        put: operations["updateBranding"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/local-sign-in": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read the organization's local sign-in switches. */
        get: operations["getLocalSignIn"];
        /** Set every local sign-in switch. A body equal to the current switches changes nothing. */
        put: operations["updateLocalSignIn"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/permissions": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List the permission catalog. */
        get: operations["listPermissions"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/providers": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List the organization's `oidc` and `oauth2` providers, by key. */
        get: operations["listProviders"];
        put?: never;
        /** Create an `oidc` or `oauth2` provider. */
        post: operations["createProvider"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/providers/{provider_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read one provider. */
        get: operations["getProvider"];
        put?: never;
        post?: never;
        /** Delete a provider no identity links a user to. To keep its identities, disable it instead. */
        delete: operations["deleteProvider"];
        options?: never;
        head?: never;
        /**
         * Change a provider's label, whether it is enabled, or members of its config. Its key and
         *     kind never change.
         */
        patch: operations["updateProvider"];
        trace?: never;
    };
    "/api/v1/admin/role-assignments": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List the role assignments at scopes where the caller holds `role.view`, by id. */
        get: operations["listRoleAssignments"];
        put?: never;
        /** Grant a role to a user at a scope. */
        post: operations["createRoleAssignment"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/role-assignments/{assignment_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        /** Remove a role assignment. */
        delete: operations["deleteRoleAssignment"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/roles": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List the roles: the built-in roles, then the organization's custom roles, each by key. */
        get: operations["listRoles"];
        put?: never;
        /** Create a custom role. */
        post: operations["createRole"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/roles/{role_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read one role. */
        get: operations["getRole"];
        put?: never;
        post?: never;
        /** Delete a custom role that has no assignments. */
        delete: operations["deleteRole"];
        options?: never;
        head?: never;
        /** Change a custom role's name or permissions; holders see the change on their next request. */
        patch: operations["updateRole"];
        trace?: never;
    };
    "/api/v1/admin/service-accounts": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List the organization's service accounts, oldest first. Keys are never listed. */
        get: operations["listServiceAccounts"];
        put?: never;
        /** Create a service account owned by the caller, and its API key. */
        post: operations["createServiceAccount"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/service-accounts/{service_account_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        /** Delete a service account; its key stops working on the next request. */
        delete: operations["deleteServiceAccount"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/users/{user_id}/enrollment-links": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Create an enrollment link for a user, replacing their earlier unused one. */
        post: operations["createEnrollmentLink"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/users/{user_id}/totp": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        /** Remove a user's TOTP and recovery codes. */
        delete: operations["removeUserTotp"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/course-files/{course_file_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read a course file. */
        get: operations["getCourseFile"];
        put?: never;
        post?: never;
        /**
         * Delete a course file no `file` item references. Its bytes stay stored until no course file
         *     has used them for 24 hours.
         */
        delete: operations["deleteCourseFile"];
        options?: never;
        head?: never;
        /**
         * Rename a course file, or move it to another folder of its course. A patch naming only current
         *     values changes nothing.
         */
        patch: operations["updateCourseFile"];
        trace?: never;
    };
    "/api/v1/courses": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * List the courses where you hold `course.view`, drafts and templates only where you also hold
         *     `course.view_all`, sorted by title, then id.
         */
        get: operations["listCourses"];
        put?: never;
        /** Create a draft course in `account_id`. */
        post: operations["createCourse"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/courses/{course_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read one course. */
        get: operations["getCourse"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        /**
         * Change a course's title, code, dates, home or completion validity. A patch naming only
         *     current values changes nothing.
         */
        patch: operations["updateCourse"];
        trace?: never;
    };
    "/api/v1/courses/{course_id}/completions": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * List the completion history of a course and its modules, newest first: one row per
         *     completion per cycle. With `progress.view` you read every learner's; with only
         *     `progress.view_observed`, the learners your active Observer enrollments in the course name.
         */
        get: operations["listCourseCompletions"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/courses/{course_id}/copy": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Copy a course into an account as a new draft, or as a template. */
        post: operations["copyCourse"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/courses/{course_id}/enrollments": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List a course's enrollments, sorted by the user's display name, then the enrollment's id. */
        get: operations["listEnrollments"];
        put?: never;
        /** Enroll a user in a course with a role, as `active`. */
        post: operations["createEnrollment"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/courses/{course_id}/files": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * List the folders and files directly in a folder of the course: folders first, then files,
         *     each sorted by name (compared as bytes).
         */
        get: operations["listCourseFiles"];
        put?: never;
        /**
         * Upload a file into a folder of the course. The body is the file itself, with its
         *     `Content-Length`; its `Content-Type` is ignored: the server reads the type from the bytes.
         *     The same bytes the organization already stores are kept once and add nothing to its storage.
         */
        post: operations["uploadCourseFile"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/courses/{course_id}/folders/rename": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Rename a folder of the course and every folder below it, in one change: either every file
         *     moves or none does.
         */
        post: operations["renameCourseFolder"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/courses/{course_id}/modules": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * The course's modules in order, each with its items in order. A caller without
         *     `course.view_all` gets only published modules and published items; a learner of the course
         *     also gets their progress on each.
         */
        get: operations["listModules"];
        put?: never;
        /** Create an unpublished module, last in the course. */
        post: operations["createModule"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/courses/{course_id}/modules/order": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        /**
         * Put the course's modules in a new order. The list must be exactly the course's current
         *     module ids; the current order changes nothing.
         */
        put: operations["reorderModules"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/courses/{course_id}/move": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Move a course, in any status, to another account of the organization. */
        post: operations["moveCourse"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/courses/{course_id}/pages": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List the course's pages by slug. */
        get: operations["listPages"];
        put?: never;
        /** Create a page with version 1 of its body. */
        post: operations["createPage"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/courses/{course_id}/progress": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * List the learners of a course with their course and module progress, sorted by display
         *     name, then user id. With `progress.view` you read every learner; with only
         *     `progress.view_observed`, the learners your active Observer enrollments in the course name.
         */
        get: operations["listCourseProgress"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/courses/{course_id}/progress/{user_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * Read one learner's progress in a course: the course, each module and each item they took a
         *     step on. With only `progress.view_observed`, only a learner you observe.
         */
        get: operations["getLearnerProgress"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/courses/{course_id}/status": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Change a course's status. Asking for the current status changes nothing. */
        post: operations["setCourseStatus"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/courses/{course_id}/video-uploads": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Start a tus upload of a video into the course. The request has no body; `Upload-Metadata`
         *     gives the video's title (`title`, or else `filename`). The video is created `uploading`,
         *     attached to the course, and the whole length counts toward the plan's `video_storage` limit
         *     from now on.
         */
        post: operations["createVideoUpload"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/courses/{course_id}/videos": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List the videos attached to the course, oldest first. */
        get: operations["listCourseVideos"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/enrollments/{enrollment_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        /**
         * Remove an enrollment. An Observer enrollment's own links go with it, and when the user keeps
         *     no other enrollment in the course whose role holds `course.participate`, so do the Observer
         *     links that name them. A user with no other such enrollment and progress in the course keeps
         *     their enrollment: conclude it instead (409 `enrollment.has_progress`).
         */
        delete: operations["deleteEnrollment"];
        options?: never;
        head?: never;
        /**
         * Conclude or reactivate an enrollment, or replace the learners an Observer enrollment
         *     observes. A change naming only current values changes nothing.
         */
        patch: operations["updateEnrollment"];
        trace?: never;
    };
    "/api/v1/items/{item_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * One item. An unpublished item, or an item of an unpublished module, answers 404 to a caller
         *     without `course.view_all`; a learner of the course also gets their progress on it.
         */
        get: operations["getItem"];
        put?: never;
        post?: never;
        /**
         * Delete an item. What it points at stays in the course. An item any learner has progress on
         *     cannot be deleted; unpublish it instead.
         */
        delete: operations["deleteItem"];
        options?: never;
        head?: never;
        /**
         * Rename, publish or unpublish an item, change an external link's URL, or set or remove its
         *     requirement. A patch naming only current values changes nothing.
         */
        patch: operations["updateItem"];
        trace?: never;
    };
    "/api/v1/items/{item_id}/move": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Move an item to a position in a module of the same course, its own included. */
        post: operations["moveItem"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/items/{item_id}/progress": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Record a step of yours on an item: `view` for a page, a file or an external link, or
         *     `mark_done` for an item whose requirement is `mark_done`. A step that meets the item's
         *     requirement completes it, and completing an item can complete its module and the course. A
         *     step that changes nothing answers your progress as it is. Completion is final.
         */
        post: operations["recordItemProgress"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/me": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read the signed-in user. */
        get: operations["getMe"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/me/appearance": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        /**
         * Set the signed-in user's theme and density; a `null` theme leaves the theme to the
         *     organization's default appearance. The answer also sets the `__Host-sc_appearance` cookie, so
         *     this browser's signed-out screens use the same choice.
         */
        put: operations["updateMyAppearance"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/me/completions": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * List your own completion history, newest first: one row per course or module completion per
         *     cycle, across your courses. Without `course.view_all` at a course, the title of a draft or
         *     template course and of an unpublished or deleted module is null.
         */
        get: operations["listMyCompletions"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/me/identities": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List the signed-in user's identities, oldest first, without the attributes providers sent. */
        get: operations["listMyIdentities"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/me/identities/{identity_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        /** Remove one of the signed-in user's identities. You must keep a way to sign in. */
        delete: operations["deleteMyIdentity"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/me/passkeys": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List your passkeys, oldest first. */
        get: operations["listMyPasskeys"];
        put?: never;
        /** Add the passkey whose registration `POST /api/v1/me/passkeys/start` began in this browser. */
        post: operations["createMyPasskey"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/me/passkeys/start": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Start adding a passkey to your account. */
        post: operations["startMyPasskey"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/me/passkeys/{passkey_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        /** Delete one of your passkeys. You must keep a way to sign in. */
        delete: operations["deleteMyPasskey"];
        options?: never;
        head?: never;
        /** Rename one of your passkeys. */
        patch: operations["updateMyPasskey"];
        trace?: never;
    };
    "/api/v1/me/password": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        /** Change the signed-in user's password. */
        put: operations["changeMyPassword"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/me/totp": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Start TOTP enrollment. */
        post: operations["startMyTotp"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/me/totp/confirm": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Confirm TOTP with a code from the app. */
        post: operations["confirmMyTotp"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/me/totp/disable": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Turn TOTP off with a current code or a recovery code. */
        post: operations["disableMyTotp"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/modules/{module_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * One module with its items in order. An unpublished module answers 404 to a caller without
         *     `course.view_all`, who also gets only its published items; a learner of the course also gets
         *     their progress.
         */
        get: operations["getModule"];
        put?: never;
        post?: never;
        /** Delete a module that has no items. */
        delete: operations["deleteModule"];
        options?: never;
        head?: never;
        /**
         * Rename, publish or unpublish a module, change its rules (unlock date, prerequisites,
         *     sequential order and completion rule) or its completion validity. A patch naming only current
         *     values changes nothing.
         */
        patch: operations["updateModule"];
        trace?: never;
    };
    "/api/v1/modules/{module_id}/items": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Create an unpublished item in the module, at `position` or last. */
        post: operations["createItem"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/modules/{module_id}/items/order": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        /**
         * Put the module's items in a new order. The list must be exactly the module's current item
         *     ids; the current order changes nothing.
         */
        put: operations["reorderItems"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/organization": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read the caller's organization. */
        get: operations["getOrganization"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        /** Rename the caller's organization. A patch without `name` changes nothing. */
        patch: operations["updateOrganization"];
        trace?: never;
    };
    "/api/v1/organization/entitlements": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * Read the organization's plan, every catalog feature with whether the plan includes it, and
         *     every catalog limit with its value.
         */
        get: operations["getOrganizationEntitlements"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/organization/settings": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read the organization settings, every key with its effective value. */
        get: operations["getOrganizationSettings"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        /**
         * Change the settings the patch names; the others keep their value. A patch naming no setting
         *     writes nothing.
         */
        patch: operations["updateOrganizationSettings"];
        trace?: never;
    };
    "/api/v1/pages/{page_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        /** Delete a page and all its versions, when no item references it. */
        delete: operations["deletePage"];
        options?: never;
        head?: never;
        /** Rename a page or change its slug. A patch naming only current values changes nothing. */
        patch: operations["updatePage"];
        trace?: never;
    };
    "/api/v1/pages/{page_id}/body": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        /**
         * Save a body as the page's new current version. The save is refused, writing nothing, when
         *     `base_version_id` is no longer the current version; a body equal to the current one writes
         *     nothing and answers the page as it is.
         */
        put: operations["savePageBody"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/pages/{page_id}/versions": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List the page's versions, newest first, without their bodies. */
        get: operations["listPageVersions"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/pages/{page_id}/versions/{version_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read one version of the page with its body. */
        get: operations["getPageVersion"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/pages/{page_id}/versions/{version_id}/restore": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Save a version's body as the page's new current version, with the save's conflict and image
         *     checks.
         */
        post: operations["restorePageVersion"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/users": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * List the organization's users, sorted by display name, then id. A caller holding `user.view`
         *     at accounts only sees the users in those accounts' subtrees: those holding a role there, or
         *     enrolled in a course there.
         */
        get: operations["listUsers"];
        put?: never;
        /** Create a user, `local` and `active`, and link the identities given to them. */
        post: operations["createUser"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/users/{user_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read one user. */
        get: operations["getUser"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        /**
         * Change a user's names, email, locale, time zone, status or manager. A patch naming nothing
         *     changes nothing.
         */
        patch: operations["updateUser"];
        trace?: never;
    };
    "/api/v1/users/{user_id}/identities": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List a user's identities, oldest first. */
        get: operations["listUserIdentities"];
        put?: never;
        /** Link an identity to a user, so they can sign in with it. */
        post: operations["createUserIdentity"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/users/{user_id}/identities/{identity_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        /** Remove an identity from a user. The user must keep a way to sign in. */
        delete: operations["deleteUserIdentity"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/video-uploads/{upload_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        /**
         * End the upload: its parts, its video and the video's attachment are deleted. From the answer
         *     on the upload is gone; its parts are deleted right after, or by the hourly cleanup if that
         *     fails. An upload whose video is already processing cannot be ended; delete the video instead.
         */
        delete: operations["deleteVideoUpload"];
        options?: never;
        /**
         * Ask how many bytes of the upload the server has: `Upload-Offset` is where the next chunk
         *     starts. A response to `HEAD` has no body, so a refusal is told by its status only.
         */
        head: operations["getVideoUploadOffset"];
        /**
         * Send the next chunk of the upload, starting at its `Upload-Offset`. A chunk is stored whole
         *     or not at all: after a dropped connection, ask for the offset with `HEAD` and send again from
         *     there. The chunk that completes the upload starts the video's processing.
         */
        patch: operations["appendVideoUpload"];
        trace?: never;
    };
    "/api/v1/videos/{video_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read a video. */
        get: operations["getVideo"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        /** Rename a video. A patch with no member changes nothing and answers the video. */
        patch: operations["updateVideo"];
        trace?: never;
    };
    "/api/v1/videos/{video_id}/retry": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Transcode again a video whose transcode failed (`video.transcode_failed`), from its upload,
         *     which the server keeps until a transcode succeeds. A video can be retried 3 times, each retry
         *     charging its transcode minutes again.
         */
        post: operations["retryVideo"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/auth/local/enrollment/passkey": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Register a passkey through an enrollment link and sign in with it. */
        post: operations["enrollPasskey"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/auth/local/enrollment/password": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Set a password through an enrollment link and sign in with it. */
        post: operations["enrollPassword"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/auth/local/enrollment/start": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Open an enrollment link: say whose it is and what it can set up, without using it. */
        post: operations["startEnrollment"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/auth/local/passkey/finish": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Finish signing in with a passkey. */
        post: operations["finishPasskeySignIn"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/auth/local/passkey/start": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Start signing in with a passkey. */
        post: operations["startPasskeySignIn"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/auth/local/password": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Sign in with an email and a password. */
        post: operations["signInWithPassword"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/auth/local/totp": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Finish signing in with a TOTP code after the password. */
        post: operations["signInWithTotp"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/auth/local/totp/enrollment": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Confirm TOTP set up during sign-in and finish signing in. */
        post: operations["enrollTotpAtSignIn"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/auth/sign-out": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * End the browser's session and clear its cookie.
         * @description Answers 204 whether or not the cookie named a session. It needs no permission and no valid
         *     session, so a browser holding an expired or unknown cookie can still clear it.
         */
        post: operations["signOut"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/auth/{provider}/callback": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * Finish a sign-in with an outside identity provider.
         * @description A browser navigation: the provider's redirect back after sign-in, with `code` and `state` or with `error` and `state`. It needs the `__Host-sc_flow` cookie of the flow the start route began, and clears it. Every refusal is a 303 to `/sign-in?error=<code>`: `auth.provider_not_found` (no enabled provider with this key), `auth.flow_invalid` (no flow cookie, a used or unknown flow, a flow for another provider, or another `state`), `auth.flow_expired` (the flow is older than 10 minutes), `auth.provider_denied` (the person declined at the provider), `auth.provider_error` (the provider reported an error, or its token, key set or userinfo answer could not be used), `auth.id_token_invalid` (the ID token failed a check), `auth.subject_missing` (the ID token's `sub` or the OAuth 2 userinfo subject is missing, empty, holds NUL or is longer than 255 characters), the admission refusals `auth.suspended`, `auth.admission_refused`, `auth.not_provisioned`, `auth.signup_closed` and `entitlement.limit_reached`, or `auth.rate_limited` (with `Retry-After`). An admitted sign-in sets `__Host-sc_session` and is a 303 to the flow's `return_to`.
         */
        get: operations["finishProviderSignIn"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/auth/{provider}/start": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * Start a sign-in with an outside identity provider.
         * @description A browser navigation. Sends the browser to the provider's authorization endpoint with `state` and a PKCE S256 challenge (for an OIDC provider also a `nonce`; for an OAuth 2 provider whose `pkce` is off, no challenge), bound to the sign-in flow whose `__Host-sc_flow` cookie this answer sets. Every refusal is a 303 to `/sign-in?error=<code>`: `auth.provider_not_found` (no enabled provider with this key), `auth.provider_unavailable` (the provider's discovery document could not be read or names another issuer, its stored config does not read, or its authorization URL is refused by the outbound URL policy) or `auth.rate_limited` (with `Retry-After`).
         */
        get: operations["startProviderSignIn"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/healthz": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** The server process is running. */
        get: operations["healthz"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/manifest.webmanifest": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * The web app manifest of the request's organization: its name, its light theme's colors and
         *     Stepclimb's default icons.
         */
        get: operations["getWebManifest"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/oauth/authorize": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * Start an app's sign-in: the authorization request of the OAuth 2.1 code flow with PKCE.
         * @description A browser navigation the app opens in the system browser. Takes `response_type=code`, `client_id`, `redirect_uri`, `code_challenge`, `code_challenge_method=S256` and `state`; other parameters are ignored and none may be given twice. An unknown `client_id` (`oauth.client_unknown`) or a `redirect_uri` the client has not registered (`oauth.redirect_uri_mismatch`) answers an HTML error page and never redirects. Any other problem is a 303 to the redirect URI with `error` (`invalid_request`, `unsupported_response_type`), `error_description`, `state` and `iss`, the issuer (RFC 9207). Without a browser session, a 303 to `/sign-in?return_to=<this request>`. Signed in, the confirmation page, whose form posts the answer to `POST /oauth/authorize`, with the `__Host-sc_flow` cookie holding the request. Over the sign-in budget, a 303 to `/sign-in?error=auth.rate_limited` with `Retry-After`.
         */
        get: operations["authorizeApp"];
        put?: never;
        /**
         * Answer an app's sign-in from the confirmation page.
         * @description The confirmation page's form: `application/x-www-form-urlencoded` with one `decision`, `allow` or `deny`, and one `request`, the id the page carries, in the browser that opened the page (the request waits in its `__Host-sc_flow` cookie) and signed in as the same user. `allow` is a 303 to the redirect URI with `code` (single use, valid 60 seconds), `state` and `iss` (the issuer, RFC 9207); `deny` a 303 with `error=access_denied`, `state` and `iss`. Every refusal is an HTML error page: `request.malformed` (no single `decision` and `request`), `auth.unauthenticated` (401, no session), `auth.flow_invalid` or `auth.flow_expired` (the request was answered already, expired, is another user's, or is not the one the answered page showed).
         */
        post: operations["answerAppAuthorization"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/oauth/revoke": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Revoke an app's refresh token, or the family of its access token.
         * @description RFC 7009 token revocation for Stepclimb's own apps: `token` (a refresh token or an access token), `token_type_hint` (optional, either kind) and `client_id`. Revoking either kind deletes every refresh token of its family, so the family's access tokens stop working too. Answers 200 whether or not the token was known. Refusals are RFC 6749 JSON with status 400: `invalid_request` or `invalid_client`.
         */
        post: operations["revokeAppToken"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/oauth/token": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Exchange an authorization code, or refresh, for an access token and a refresh token.
         * @description The token endpoint of the OAuth 2.1 server for Stepclimb's own apps (public clients, no client secret). `grant_type=authorization_code` with `code`, `redirect_uri`, `client_id` and `code_verifier` exchanges a code from `/oauth/authorize` once; `grant_type=refresh_token` with `refresh_token` and `client_id` rotates a refresh token: the presented one stops working, and presenting a used code or a rotated refresh token again revokes every token of its family. Refusals are RFC 6749 JSON with status 400: `invalid_request`, `invalid_client`, `invalid_grant` (any code or refresh token that does not pass, and a suspended user's) or `unsupported_grant_type`. Every answer carries `Cache-Control: no-store`.
         */
        post: operations["requestAppToken"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/readyz": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** The server can take traffic: the database answered within two seconds. */
        get: operations["readyz"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/theme.css": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** The stylesheet of the request's organization: its saved branding's, or the default brand's. */
        get: operations["getThemeCss"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
}
export type webhooks = Record<string, never>;
export interface components {
    schemas: {
        /** @description An account of the caller's organization. */
        Account: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /**
             * Format: int32
             * @description 0 for the root, 1 for its children, and so on; at most 16.
             */
            depth: number;
            has_children: boolean;
            /** Format: uuid */
            id: string;
            name: string;
            /**
             * Format: uuid
             * @description `null` for the root only. Shown even when the parent is not visible to the caller.
             */
            parent_id: string | null;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            updated_at: string;
        };
        /** @description The new parent of a moved account. */
        AccountMove: {
            /**
             * Format: uuid
             * @description The new parent; you must hold `account.manage` at a scope containing it. The account's
             *     current parent changes nothing.
             */
            parent_id: string;
        };
        /**
         * @description A change to an account; an omitted member is left as it is. A member sent as `null` is
         *     malformed: omit it instead.
         */
        AccountPatch: {
            /** @description Trimmed; 1 to 200 characters. */
            name?: string;
        };
        /** @description Holds when every one of its 1 to 50 rules holds. Groups nest at most 4 deep. */
        AdmissionAll: {
            all: components["schemas"]["AdmissionRule"][];
        };
        /** @description Holds when at least one of its 1 to 50 rules holds. Groups nest at most 4 deep. */
        AdmissionAny: {
            any: components["schemas"]["AdmissionRule"][];
        };
        /**
         * @description A test of one mapped attribute. Rules see attributes exactly as the provider's claims map
         *     them, verified or not: an attribute mapped from an email claim holds the address the
         *     provider sent even when it did not vouch for it. To rely on such a value, map the
         *     provider's verification claim as an attribute too and require it alongside, as in
         *     `{"attribute": "email_verified", "op": "equals", "value": true}`.
         */
        AdmissionCondition: {
            /**
             * @description The attribute's name, as the provider's `claims.attributes` maps it; 1 to 1,000
             *     characters.
             */
            attribute: string;
            op: components["schemas"]["AdmissionOp"];
            value?: null | components["schemas"]["AdmissionValue"];
        };
        /**
         * @description What a condition asks of its attribute.
         * @enum {string}
         */
        AdmissionOp: "equals" | "in" | "not_in" | "exists" | "matches";
        /**
         * @description One admission rule: a condition on one mapped attribute, or a group of rules. An attribute
         *     that is absent or `null` fails its condition, whatever the `op`; one of a type the rule
         *     cannot compare refuses the sign-in.
         */
        AdmissionRule: components["schemas"]["AdmissionCondition"] | components["schemas"]["AdmissionAny"] | components["schemas"]["AdmissionAll"];
        /**
         * @description The rules every sign-in through an outside provider must pass (Q3 (a)): every rule of the
         *     organization's list and of the provider's own list. A sign-in with a local account runs no
         *     rules. A refused person is answered `auth.admission_refused`.
         */
        AdmissionRules: {
            /**
             * @description The rules every outside provider's sign-ins must pass. Their attributes must be ones
             *     some provider maps; a provider that does not map one fails the rule.
             */
            organization: components["schemas"]["AdmissionRule"][];
            /**
             * @description Each outside provider of the organization, enabled or not, by key, with the rules its
             *     sign-ins must pass besides the organization's; `[]` when it has none.
             */
            providers: {
                [key: string]: components["schemas"]["AdmissionRule"][];
            };
        };
        /**
         * @description The rule lists to replace: each list named replaces the stored one whole, and a list not
         *     named keeps its rules. At most 50 rules per list.
         */
        AdmissionRulesPatch: {
            /** @description The organization's list. */
            organization?: components["schemas"]["AdmissionRule"][];
            /** @description Provider lists by provider key; a provider not named keeps its list. `[]` empties one. */
            providers?: {
                [key: string]: components["schemas"]["AdmissionRule"][];
            };
        };
        /** @description One value of an `in` or `not_in` list. */
        AdmissionScalar: string | number | boolean;
        /** @description Who admission lets in. */
        AdmissionSettings: {
            rules: components["schemas"]["AdmissionRules"];
            /**
             * @description Whether a person the organization has no user for is created as a user when they first
             *     sign in through an identity provider. Off, they are refused with `auth.signup_closed`.
             */
            signup_allowed: boolean;
            /**
             * @description Whether only users already provisioned may sign in: on, a person with no user of the
             *     organization (no linked identity, no email match) is refused with `auth.not_provisioned`
             *     even when sign-up is allowed; `false` unless set.
             */
            sync_only: boolean;
        };
        /**
         * @description A change to the admission switches and rules: send only what you change. An absent member
         *     keeps its stored value, and `{}` changes nothing. A member sent as `null` is malformed: omit
         *     it instead.
         */
        AdmissionSettingsPatch: {
            rules?: components["schemas"]["AdmissionRulesPatch"];
            /**
             * @description Whether a person the organization has no user for is created as a user when they first
             *     sign in through an identity provider.
             */
            signup_allowed?: boolean;
            /**
             * @description Whether only users already provisioned may sign in: on, a person with no user of the
             *     organization (no linked identity, no email match) is refused with `auth.not_provisioned`
             *     even when sign-up is allowed; `false` unless set.
             */
            sync_only?: boolean;
        };
        /** @description The admission switches as the organization settings show them. */
        AdmissionSwitches: {
            /**
             * @description Whether a person the organization has no user for is created as a user when they first
             *     sign in through an identity provider; `false` unless set. Off, they are refused with
             *     `auth.signup_closed`.
             */
            signup_allowed: boolean;
            /** @description Whether only users already provisioned may sign in; `false` unless set. */
            sync_only: boolean;
        };
        /** @description A condition's value. */
        AdmissionValue: string | number | boolean | components["schemas"]["AdmissionScalar"][];
        /** @description The organization's admission webhook, without its secret. */
        AdmissionWebhook: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /**
             * @description Whether admission calls it. Off, sign-ins skip the webhook. A new webhook is off until
             *     enabled.
             */
            enabled: boolean;
            /**
             * @description Where admission POSTs each sign-in through an outside provider. Not a secret: every
             *     `admission.manage` holder reads it and the audit log records it.
             */
            url: string;
        };
        /**
         * @description The JSON body Stepclimb POSTs to an organization's admission webhook at each sign-in through
         *     an outside identity provider. It is not a Stepclimb endpoint. The request carries the
         *     Standard Webhooks headers `webhook-id`, `webhook-timestamp` (Unix seconds) and
         *     `webhook-signature` (`v1,` and the base64 HMAC-SHA256 of `id.timestamp.body`, keyed with the
         *     webhook's `whsec_` secret); verify them with a Standard Webhooks library. Answer within 3
         *     seconds with status 200 and an `AdmissionWebhookReply`.
         */
        AdmissionWebhookCall: {
            /** @description The attributes the provider's claims mapped. */
            attributes: Record<string, never>;
            /** @description The key of the identity provider the person signed in with. */
            provider: string;
            /** @description The provider's identifier for the person. */
            subject: string;
        };
        /**
         * @description A change to the webhook: send only what you change. An absent member keeps its stored
         *     value; creating the webhook needs `url`, and `enabled` is then `false` unless sent, so the
         *     receiver can be given the secret first. A member sent as `null` is malformed: omit it
         *     instead. Without the plan's `admission_webhook` feature, only `{"enabled": false}` (or `{}`)
         *     is accepted.
         */
        AdmissionWebhookPatch: {
            /** @description Whether admission calls the webhook. */
            enabled?: boolean;
            /**
             * @description An absolute `https` URL, at most 2,048 characters, with no fragment, user name or
             *     password, kept exactly as sent. It is not a secret: it is returned to every
             *     `admission.manage` holder and written to the audit log. Authenticate the receiver with
             *     the signing secret, never with a token in a query parameter.
             */
            url?: string;
        };
        /**
         * @description The reply an admission webhook gives Stepclimb, with status 200, at most 64 KiB. Any other
         *     status, a redirect, a body that is not this object, or no answer within 3 seconds refuses
         *     the sign-in with `auth.admission_unavailable`. Members not listed here are ignored.
         */
        AdmissionWebhookReply: {
            /** @description `true` admits the sign-in; `false` refuses it with `auth.admission_refused`. */
            allow: boolean;
            /**
             * @description Attributes merged over the provider's mapped attributes before they are stored; these
             *     keys win. An object when present, never `null`, with no NUL character (`\u0000`) in any
             *     key or string.
             */
            attributes?: Record<string, never>;
            /**
             * @description The groups the user is in, by key: at most 1,000 strings of 1 to 255 characters with no
             *     control character, no Unicode format character (category Cf) and neither U+2028 nor
             *     U+2029, never `null`. They replace the memberships earlier replies gave the user, so a
             *     group left out is removed; an absent `groups` is no groups. A group the organization
             *     does not have yet is created, named by its key, while it has fewer than 10,000 groups;
             *     past that, a key naming no group is left out. Keys are matched exactly.
             */
            groups?: string[];
        };
        /** @description A new signing secret, shown once. */
        AdmissionWebhookSecret: {
            /**
             * @description The Standard Webhooks signing secret, `whsec_` and 32 bytes in base64. The old one stops
             *     signing at once: sign-ins through outside providers are refused until the receiver has
             *     this one.
             */
            secret: string;
        };
        /**
         * @description An appearance every brand has.
         * @enum {string}
         */
        Appearance: "light" | "dark" | "high-contrast";
        /**
         * @description Who owns an assignment: an admin of this organization (`local`), or the identity provider,
         *     sync API, SCIM or connector that wrote it, in which case it cannot be removed here.
         * @enum {string}
         */
        AssignmentSource: "local" | "idp" | "sync" | "scim" | "connector";
        /** @description `AuthenticatorSelectionCriteria`. */
        AuthenticatorSelection: {
            authenticatorAttachment?: string;
            requireResidentKey: boolean;
            residentKey?: string;
            /** @description `required`: the authenticator checks a PIN or biometric. */
            userVerification: string;
        };
        /**
         * @description The organization's branding: the saved inputs (the default brand's when it has saved none),
         *     the generator version, the hash of the stylesheet `/theme.css` serves, and the contrast
         *     report of its light, dark and high-contrast themes.
         */
        Branding: {
            /** @description The SHA-256 of the stylesheet, lowercase hex. */
            css_hash: string;
            fallback: null | components["schemas"]["BrandingFallback"];
            /**
             * Format: int32
             * @description The version of the theme generator that wrote the stylesheet.
             */
            generator: number;
            inputs: components["schemas"]["SavedInputs"];
            report: components["schemas"]["ContrastReport"][];
        };
        /**
         * @description Why `/theme.css` serves the default brand's stylesheet for saved branding: a release with
         *     another theme generator re-rendered the saved inputs, and they fail its contrast report or its
         *     input checks. Saving branding again replaces it.
         */
        BrandingFallback: {
            /**
             * @description The reports of the saved inputs' themes under this generator, failing pairs first; empty
             *     when the generator refused the inputs or could not build their themes.
             */
            report: components["schemas"]["ContrastReport"][];
        };
        /**
         * @description The branding inputs. The whole set is replaced; an omitted or `null` choice takes its
         *     default. A value that is not a choice is a 422, not a 400.
         */
        BrandingInputs: {
            corner_style?: null | components["schemas"]["CornerStyle"];
            default_appearance?: null | components["schemas"]["ThemeChoice"];
            /** @description The primary color: a CSS hex color (`#500E0E`, `#f00`) or `oklch(L C H)`, opaque. */
            primary: string;
            reading_font?: null | components["schemas"]["ReadingFont"];
            ui_font?: null | components["schemas"]["UiFont"];
        };
        /**
         * @description A problem details body with the contrast report: `branding.contrast_failed` carries
         *     `report`, every other code leaves it out.
         */
        BrandingProblem: components["schemas"]["Problem"] & {
            /** @description On `branding.contrast_failed`, every appearance's report, failing pairs first. */
            report?: components["schemas"]["ContrastReport"][] | null;
        };
        /** @description A chapter mark of a video. */
        Chapter: {
            /**
             * Format: double
             * @description Seconds from the start.
             */
            at_s: number;
            title: string;
        };
        /** @description A right password whose account also needs a TOTP code: no session exists yet. */
        CodeRequired: {
            /** @description Send the code to `POST /auth/local/totp` in this browser within 10 minutes. */
            next: components["schemas"]["NextStep"];
        };
        /** @description One completion of a course or a module, in one cycle. */
        Completion: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            completed_at: string;
            /**
             * Format: int32
             * @description 1 for the user's first completion of the target, then 2, and so on.
             */
            cycle: number;
            /**
             * Format: date-time
             * @description RFC 3339, UTC: when the completion expired; null while it is valid.
             */
            expired_at: string | null;
            /**
             * Format: date-time
             * @description RFC 3339, UTC: when the completion stops being valid; null when it does not expire.
             */
            expires_at: string | null;
            /** Format: uuid */
            id: string;
            target: components["schemas"]["CompletionTarget"];
            user: components["schemas"]["ProgressUser"];
        };
        /**
         * @description Whether all or one of a module's requirements complete it.
         * @enum {string}
         */
        CompletionRule: "all" | "one";
        /** @description The course or module a completion is of. */
        CompletionTarget: {
            /** Format: uuid */
            id: string;
            /**
             * @description The course's or module's title now; null for a module deleted since, and on your own
             *     history where you cannot read it now without `course.view_all` (a draft or template
             *     course, an unpublished module).
             */
            title: string | null;
            type: components["schemas"]["CompletionTargetType"];
        };
        /**
         * @description What a completion is of.
         * @enum {string}
         */
        CompletionTargetType: "course" | "module";
        /** @description One measured pair: `foreground` on `background`. */
        ContrastPair: {
            /** @description A color token, such as `bg-canvas`. */
            background: string;
            /** @description A color token, such as `fg-soft`. */
            foreground: string;
            /**
             * Format: double
             * @description 4.5 for text and 3 for other pairs; 7 and 4.5 in high contrast.
             */
            minimum: number;
            pass: boolean;
            /**
             * Format: double
             * @description The WCAG 2 contrast ratio on the displayed 8-bit sRGB colors, 1 to 21, unrounded.
             */
            ratio: number;
        };
        /** @description One appearance's contrast report. */
        ContrastReport: {
            appearance: components["schemas"]["Appearance"];
            pairs: components["schemas"]["ContrastPair"][];
            /** @description True when every pair passes. */
            pass: boolean;
            warnings: components["schemas"]["ContrastWarning"][];
        };
        /**
         * @description Something to look at that does not fail the theme. `brand-near-danger`: the brand's hue is
         *     within 30 degrees of `danger`'s.
         */
        ContrastWarning: {
            code: string;
            message: string;
        };
        /**
         * @description The corner style: `square` sets the `xs`, `sm` and `md` radii to 0, `round` doubles them.
         * @enum {string}
         */
        CornerStyle: "square" | "default" | "round";
        /** @description A course of the caller's organization. */
        Course: {
            /** Format: uuid */
            account_id: string;
            code: string;
            /**
             * Format: int32
             * @description How many months a completion of the course stays valid; null for ever. A change applies
             *     to completions recorded afterwards.
             */
            completion_valid_months: number | null;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /**
             * Format: date-time
             * @description RFC 3339, UTC. Shown to learners and staff; it opens and closes nothing.
             */
            end_at: string | null;
            /** Format: uuid */
            id: string;
            /**
             * @description The catalog keys grantable at course scope that you hold at this course, sorted: what
             *     the app may offer you here. The server checks every request itself.
             */
            permissions: string[];
            progress?: null | components["schemas"]["CourseProgress"];
            settings: components["schemas"]["CourseSettings"];
            /**
             * Format: date-time
             * @description RFC 3339, UTC. Shown to learners and staff; it opens and closes nothing.
             */
            start_at: string | null;
            status: components["schemas"]["CourseStatus"];
            /** @description A template is never published and takes no learners. */
            template: boolean;
            title: string;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            updated_at: string;
        };
        /** @description The `data` of `course.completed` (B11). */
        CourseCompletedData: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            completed_at: string;
            /** Format: uuid */
            course_id: string;
            /** Format: uuid */
            user_id: string;
        };
        /** @description `course.completed`, schema version 1: a learner completed a course. */
        CourseCompletedEvent: {
            data: components["schemas"]["CourseCompletedData"];
            /**
             * Format: date-time
             * @description RFC 3339, UTC: when the change happened.
             */
            timestamp: string;
            type: components["schemas"]["CourseCompletedType"];
        };
        /**
         * @description The type of a `course.completed` event.
         * @enum {string}
         */
        CourseCompletedType: "course.completed";
        /** @description A copy to make of a course, as a draft. */
        CourseCopy: {
            /**
             * Format: uuid
             * @description The account the copy belongs to; you must hold `course.create` at a scope containing it.
             */
            account_id: string;
            /** @description Trimmed; 1 to 50 characters; unique in the organization, ignoring ASCII case. */
            code: string;
            /**
             * @description Whether the copy is a template. Omitted or `false`, the copy is an ordinary course, also
             *     when the source is a template.
             */
            template?: boolean;
            /** @description Trimmed; 1 to 200 characters. The source's title when omitted. */
            title?: string;
        };
        /** @description A file in a folder of a course. */
        CourseFile: {
            /** Format: uuid */
            course_id: string;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /** @description `/` or a path such as `/handouts/week-1`, NFC-normalized. */
            folder: string;
            /** Format: uuid */
            id: string;
            /**
             * @description The type the server read from the file's first bytes; `application/octet-stream` when it
             *     recognized none. Never the uploader's `Content-Type`.
             */
            mime: string;
            /** @description NFC-normalized; unique in the folder. */
            name: string;
            /** @description The SHA-256 of the bytes as the server computed it, 64 lowercase hexadecimal digits. */
            sha256: string;
            /**
             * Format: int64
             * @description In bytes, as the server counted them.
             */
            size: number;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            updated_at: string;
        };
        /** @description A change to a course file; an omitted member is left as it is. */
        CourseFilePatch: {
            /** @description The folder to move the file to, in the same course. */
            folder?: string;
            /** @description The file's new name. */
            name?: string;
        };
        /** @description What a learner sees first when opening the course. */
        CourseHome: {
            kind: components["schemas"]["HomeKind"];
        };
        /** @description The account to move a course to. */
        CourseMove: {
            /**
             * Format: uuid
             * @description You must hold `course.create` at a scope containing it. The course's own account
             *     changes nothing.
             */
            account_id: string;
        };
        /**
         * @description A change to a course; an omitted member is left as it is. `null` clears `start_at`, `end_at`
         *     or `completion_valid_months` and is malformed for every other member.
         */
        CoursePatch: {
            /** @description Trimmed; 1 to 50 characters; unique in the organization, ignoring ASCII case. */
            code?: string;
            /**
             * Format: int64
             * @description How many months a completion of the course stays valid, 1 to 120; `null` for ever. Needs
             *     `course.manage`, as every member here does. Completions recorded before the change keep
             *     their expiry date.
             */
            completion_valid_months?: number | null;
            /**
             * Format: date-time
             * @description RFC 3339 with an offset, from 1970 up to 2200, not before `start_at`; `null` clears it.
             */
            end_at?: string | null;
            settings?: components["schemas"]["CourseSettingsPatch"];
            /**
             * Format: date-time
             * @description RFC 3339 with an offset, from 1970 up to 2200; `null` clears it.
             */
            start_at?: string | null;
            /** @description Trimmed; 1 to 200 characters. */
            title?: string;
        };
        /**
         * @description Your roll-up for a course: its modules that have a published requirement, and how many of
         *     them you completed.
         */
        CourseProgress: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC: when you completed the course; null until then. Never cleared.
             */
            completed_at: string | null;
            /** Format: int32 */
            completed_count: number;
            /**
             * Format: date-time
             * @description RFC 3339, UTC: when that completion stops being valid; null while the course is not
             *     complete or when its completion does not expire.
             */
            expires_at: string | null;
            /** Format: int32 */
            required_count: number;
        };
        /** @description A course's settings. */
        CourseSettings: {
            home: components["schemas"]["CourseHome"];
        };
        /** @description A change to a course's settings; an omitted member is left as it is. */
        CourseSettingsPatch: {
            home?: components["schemas"]["CourseHome"];
        };
        /**
         * @description A course's status.
         * @enum {string}
         */
        CourseStatus: "draft" | "published" | "archived";
        /** @description The status to change a course to. */
        CourseStatusChange: {
            status: components["schemas"]["CourseStatus"];
        };
        /** @description A service account just created, with its API key. Its `Debug` output leaves the key out. */
        CreatedServiceAccount: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /** Format: uuid */
            id: string;
            /**
             * @description The API key, `sc_<random>_<checksum>`. This is the only answer that carries it: store it
             *     now. Send it as `Authorization: Bearer <key>`.
             */
            key: string;
            kind: components["schemas"]["ServiceAccountKind"];
            /**
             * Format: date-time
             * @description Always `null`: the key has not been used.
             */
            last_used_at: string | null;
            name: string;
            /** Format: uuid */
            owner_user_id: string;
            scopes: string[];
        };
        /** @description `PublicKeyCredentialDescriptorJSON`. */
        CredentialDescriptor: {
            /** @description The credential id, base64url, unpadded. */
            id: string;
            transports?: string[];
            type: string;
        };
        /** @description `PublicKeyCredentialParameters`. */
        CredentialParameter: {
            /**
             * Format: int32
             * @description A COSE algorithm identifier, such as -7 (ES256).
             */
            alg: number;
            type: string;
        };
        /** @description The version that made a page save conflict: the page's current one (M1 spec 07 B3). */
        CurrentVersion: {
            /**
             * Format: uuid
             * @description The user who saved it.
             */
            author_id: string;
            /** Format: uuid */
            id: string;
            /** Format: int32 */
            number: number;
        };
        /**
         * @description The density a user chose. `compact` sets the shorter control heights.
         * @enum {string}
         */
        DensityChoice: "comfortable" | "compact";
        /** @description The request organization's discovery document. */
        Discovery: {
            /** @description The organization's name. */
            name: string;
            /** @description The OAuth 2.1 endpoints Stepclimb's own apps sign in through. */
            oauth: components["schemas"]["DiscoveryOauth"];
            /**
             * @description The sign-in methods the organization offers, in the order a sign-in screen lists them:
             *     local sign-in first when it is offered, then the outside providers by key. Empty when it
             *     offers none.
             */
            sign_in: components["schemas"]["SignInMethod"][];
            /** @description The organization's theme. */
            theme: components["schemas"]["DiscoveryTheme"];
        };
        /** @description The OAuth 2.1 server's issuer and endpoints, as absolute URLs on the public URL. */
        DiscoveryOauth: {
            /**
             * @description `<public URL>/oauth/authorize`, which the app opens in the system browser with its PKCE
             *     challenge.
             */
            authorization_endpoint: string;
            /**
             * @description The server's issuer identifier, the public URL. Every answer the authorization endpoint
             *     sends to a redirect URI carries it as `iss` (RFC 9207); an app checks the two agree, and
             *     that this value is the URL it fetched the document from.
             */
            issuer: string;
            /** @description `<public URL>/oauth/revoke`, where the app ends its sign-in (RFC 7009). */
            revocation_endpoint: string;
            /** @description `<public URL>/oauth/token`, where the app exchanges its code and refreshes its tokens. */
            token_endpoint: string;
        };
        /** @description The organization's theme, as the apps load it. */
        DiscoveryTheme: {
            /**
             * @description The absolute URL of the organization's stylesheet, `<public URL>/theme.css?v=<SHA-256>`,
             *     which is cached for a year; a branding change gives it another hash.
             */
            stylesheet: string;
        };
        /** @description How host sites link into the organization's courses. */
        Embedding: {
            /**
             * @description The origins a deep link's `return` URL may point at for the web app to show a "Back to"
             *     link, each `https://host[:port]` in lowercase without a path; empty unless set.
             */
            return_origins: string[];
        };
        /** @description The whole new `embedding`: each member is required. */
        EmbeddingPatch: {
            /**
             * @description At most 20 entries, each `https://host[:port]` without a path, in any letter case; a
             *     host outside ASCII is written in its `xn--` form. Stored in lowercase, each once.
             */
            return_origins: string[];
        };
        /**
         * @description A local method an enrollment link can set up.
         * @enum {string}
         */
        EnrollMethod: "passkey" | "password";
        /** @description TOTP is on and the user is signed in; the recovery codes are shown once. */
        EnrolledSignedIn: {
            /** @description Ten single-use codes that stand in for a TOTP code when the app is lost. */
            recovery_codes: string[];
            /** @description The path on this site to open next. */
            return_to: string;
        };
        /** @description The enrolled user, as much as a roster needs. */
        EnrolledUser: {
            display_name: string;
            /** Format: uuid */
            id: string;
        };
        /** @description A user's enrollment in a course with a role. */
        Enrollment: {
            /** Format: uuid */
            course_id: string;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /** @description The owning system's id for the enrollment; `null` for a `local` one. */
            external_id: string | null;
            /** Format: uuid */
            id: string;
            /**
             * @description The learners this enrollment observes, sorted by id: present when its role holds
             *     `progress.view_observed` (an Observer), absent otherwise.
             */
            observed_user_ids?: string[];
            /** Format: uuid */
            role_id: string;
            source: components["schemas"]["EnrollmentSource"];
            status: components["schemas"]["EnrollmentStatus"];
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            updated_at: string;
            user: components["schemas"]["EnrolledUser"];
        };
        /** @description An enrollment link, returned once: the only copy of its token. */
        EnrollmentLink: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            expires_at: string;
            /**
             * @description The link to hand to the user. It holds a secret: anyone with it can set up a way to
             *     sign in as the user until it is used, replaced or expires.
             */
            url: string;
        };
        /** @description A change to an enrollment; an absent member keeps its value. */
        EnrollmentPatch: {
            /**
             * @description The whole new list of learners the enrollment observes, for a role holding
             *     `progress.view_observed`: 1 to 100 distinct learners of the course. Needs `progress.view`
             *     at the course.
             */
            observed_user_ids?: string[];
            /** @description Reactivating an enrollment that observes learners needs `progress.view` at the course. */
            status?: components["schemas"]["EnrollmentStatus"];
        };
        /** @description A right password whose account must set up TOTP first: no session exists yet. Shown once. */
        EnrollmentRequired: {
            /**
             * @description `totp_enroll`: send a code from the app to `POST /auth/local/totp/enrollment` in this
             *     browser within 10 minutes.
             */
            next: components["schemas"]["NextStep"];
            /** @description The URI to show as a QR code for the authenticator app. */
            otpauth_uri: string;
            /** @description The secret in base32, for typing into the app instead. */
            secret: string;
        };
        /**
         * @description Who owns an enrollment: an admin of this organization (`local`), or the identity provider,
         *     sync API, SCIM or connector that wrote it, in which case it cannot be changed here.
         * @enum {string}
         */
        EnrollmentSource: "local" | "idp" | "sync" | "scim" | "connector";
        /** @description The token from an enrollment link. */
        EnrollmentStart: {
            /** @description The text after `#token=` in the link. */
            token: string;
        };
        /** @description What an enrollment link sets up. */
        EnrollmentStarted: {
            /** @description The email the user will sign in with. */
            email: string;
            /** @description The methods the organization lets the user set up, passkey first. Never empty. */
            methods: components["schemas"]["EnrollMethod"][];
            /**
             * @description When `methods` lists `passkey`: the creation options for the passkey, bound to the
             *     sign-in flow whose cookie this answer sets. Finish with `POST
             *     /auth/local/enrollment/passkey` in this browser within 10 minutes.
             */
            passkey_options?: components["schemas"]["PasskeyCreationOptions"];
        };
        /**
         * @description An enrollment's status: `active` grants the role at the course, `concluded` grants nothing
         *     and is kept as history.
         * @enum {string}
         */
        EnrollmentStatus: "active" | "concluded";
        /** @description The organization's plan and what it allows: every feature and limit of the server's catalogs. */
        Entitlements: {
            /** @description Every catalog feature, by key, with whether the plan includes it. */
            features: {
                [key: string]: boolean;
            };
            /** @description Every catalog limit, by key, with the most units the plan allows, or `null` for no limit. */
            limits: {
                [key: string]: number | null;
            };
            plan: components["schemas"]["PlanRef"];
        };
        /** @description One rule a request body broke, in a 422's `errors`. */
        FieldError: {
            /** @description The stable code of the rule, such as `settings.unknown_key`. */
            code: string;
            /** @description A JSON Pointer (RFC 6901) to the member that broke the rule, such as `/name`. */
            field: string;
        };
        /** @description One entry of a folder: a folder directly inside it, or a file in it. */
        FolderEntry: {
            /** @enum {string} */
            kind: "folder";
            /** @description Its last segment. */
            name: string;
            /** @description The folder's whole path, such as `/handouts/week-1`. */
            path: string;
        } | {
            file: components["schemas"]["CourseFile"];
            /** @enum {string} */
            kind: "file";
        };
        /** @description A folder to rename, with everything below it. */
        FolderRename: {
            /** @description The folder's path now; not `/`. */
            from: string;
            /** @description Its new path; not `/`. */
            to: string;
        };
        /** @description How many course files a folder rename moved. */
        FolderRenamed: {
            /** Format: int64 */
            moved: number;
        };
        /**
         * @description A scope a grant or a role assignment applies at: the organization, an account or a course,
         *     by id. An organization scope's `id` is the organization's id.
         */
        GrantScope: {
            /** Format: uuid */
            id: string;
            type: components["schemas"]["ScopeType"];
        };
        /** @description A probe's answer. */
        Health: {
            status: components["schemas"]["HealthStatus"];
        };
        /**
         * @description `ok` when the probe passed, `unavailable` when it did not.
         * @enum {string}
         */
        HealthStatus: "ok" | "unavailable";
        /**
         * @description The kind of a course's home: `modules`, the module list.
         * @enum {string}
         */
        HomeKind: "modules";
        /** @description A user's identity as an admin sees it. */
        Identity: {
            /**
             * @description The attributes the provider sent at the latest sign-in, as mapped by the provider's
             *     configuration; `{}` before the first sign-in. Admission rules read them.
             */
            attributes: Record<string, never>;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /** Format: uuid */
            id: string;
            /** @description The key of the identity provider. */
            provider_key: string;
            /** @description The provider's id for the person, exactly as the provider sends it. */
            subject: string;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            updated_at: string;
        };
        /** @description An item of a module. */
        Item: {
            /** Format: uuid */
            course_id: string;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /** Format: uuid */
            id: string;
            kind: components["schemas"]["ItemKind"];
            lock: null | components["schemas"]["Lock"];
            /** Format: uuid */
            module_id: string;
            /**
             * Format: int32
             * @description 0-based, dense within the module.
             */
            position: number;
            progress?: null | components["schemas"]["ItemProgress"];
            published: boolean;
            /**
             * Format: uuid
             * @description What the item points at; only for kinds that point at a row. Left out while the item is
             *     locked for you (`lock` is not null).
             */
            ref_id?: string;
            requirement: null | components["schemas"]["Requirement"];
            title: string;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            updated_at: string;
            /** @description Only for `external_link`, and left out as `ref_id` is. */
            url?: string;
        };
        /** @description The `data` of `item.completed` (B11): ids and facts, never a name, an email or a title. */
        ItemCompletedData: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            completed_at: string;
            /** Format: uuid */
            course_id: string;
            /** Format: uuid */
            item_id: string;
            item_kind: components["schemas"]["ItemKind"];
            /** Format: uuid */
            module_id: string;
            requirement_kind: components["schemas"]["RequirementKind"];
            /** Format: uuid */
            user_id: string;
        };
        /** @description `item.completed`, schema version 1: a learner completed an item. */
        ItemCompletedEvent: {
            data: components["schemas"]["ItemCompletedData"];
            /**
             * Format: date-time
             * @description RFC 3339, UTC: when the change happened.
             */
            timestamp: string;
            type: components["schemas"]["ItemCompletedType"];
        };
        /**
         * @description The type of an `item.completed` event.
         * @enum {string}
         */
        ItemCompletedType: "item.completed";
        /**
         * @description What an item points at.
         * @enum {string}
         */
        ItemKind: "header" | "external_link" | "file" | "page" | "video" | "quiz" | "activity";
        /** @description Where to move an item. */
        ItemMove: {
            /**
             * Format: uuid
             * @description A module of the item's course, its own included.
             */
            module_id: string;
            /**
             * Format: int64
             * @description 0 to the destination's item count, not counting the moved item.
             */
            position: number;
        };
        /** @description Every item id of the module, in the new order. */
        ItemOrder: {
            item_ids: string[];
        };
        /** @description A change to an item; an omitted member is left as it is. The kind and `ref_id` never change. */
        ItemPatch: {
            /** @description Whether learners see the item. */
            published?: boolean;
            requirement?: null | components["schemas"]["Requirement"];
            /** @description Trimmed; 1 to 200 characters. */
            title?: string;
            /** @description For an external link only. */
            url?: string;
        };
        /** @description Your progress on one item. */
        ItemProgress: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC: when you completed it; null until then.
             */
            completed_at: string | null;
            /** Format: uuid */
            item_id: string;
            /**
             * Format: date-time
             * @description RFC 3339, UTC: your first step on the item.
             */
            started_at: string;
            state: components["schemas"]["ProgressState"];
        };
        /**
         * @description One learner of a course and their progress. `modules` lists the modules the learner
         *     completed an item of, in course order; without `course.view_all` at the course, published
         *     modules only.
         */
        LearnerProgress: {
            course: null | components["schemas"]["CourseProgress"];
            /**
             * @description `active` when any of the learner's enrollments in the course that make them a learner
             *     is, otherwise `concluded`.
             */
            enrollment_status: components["schemas"]["EnrollmentStatus"];
            modules: components["schemas"]["ModuleProgress"][];
            user: components["schemas"]["ProgressUser"];
        };
        /**
         * @description One learner of a course with their progress on each item they took a step on, in course
         *     order; without `course.view_all` at the course, published items in published modules only.
         */
        LearnerProgressDetail: {
            course: null | components["schemas"]["CourseProgress"];
            enrollment_status: components["schemas"]["EnrollmentStatus"];
            items: components["schemas"]["ItemProgress"][];
            modules: components["schemas"]["ModuleProgress"][];
            user: components["schemas"]["ProgressUser"];
        };
        /**
         * @description A local sign-in method.
         * @enum {string}
         */
        LocalMethod: "passkey" | "password" | "totp" | "magic_link";
        /** @description The local sign-in method switches as the organization settings show them. */
        LocalMethods: {
            magic_link: boolean;
            passkey: boolean;
            password: boolean;
            totp: boolean;
            /** @description Every password sign-in needs a TOTP code. */
            totp_required: boolean;
        };
        /**
         * @description The organization's local sign-in: whether it is on at all, and each method's switch. A method
         *     is offered only when `enabled` and its own switch are both on.
         */
        LocalSignIn: {
            /** @description Local sign-in as a whole; off, no local method is offered. */
            enabled: boolean;
            /** @description Sign-in with a link sent by email. Off until the server can send email. */
            magic_link: boolean;
            /** @description Sign-in with a passkey. */
            passkey: boolean;
            /** @description Sign-in with a password. */
            password: boolean;
            /** @description A TOTP code as the second step after a password. */
            totp: boolean;
            /**
             * @description Every password sign-in needs a TOTP code: a user without TOTP sets it up at their next
             *     password sign-in, before they get a session. Needs `password` and `totp` on. Turning it
             *     off keeps the TOTP users already set up.
             */
            totp_required: boolean;
        };
        /** @description Why a module or item is locked for you: every condition that holds, never empty. */
        Lock: {
            reasons: components["schemas"]["LockReason"][];
        };
        /** @description One condition that keeps a module or item locked. */
        LockReason: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            at: string;
            /** @enum {string} */
            kind: "unlock_at";
        } | {
            /** @enum {string} */
            kind: "prerequisites";
            module_ids: string[];
        } | {
            /** @enum {string} */
            kind: "module_locked";
        } | {
            /** Format: uuid */
            item_id: string;
            /** @enum {string} */
            kind: "sequential";
        };
        /** @description One icon of the manifest. */
        ManifestIcon: {
            /** @description `maskable` for the icon drawn to be cropped to the platform's shape; absent otherwise. */
            purpose?: string;
            /** @description `<width>x<height>` in px. */
            sizes: string;
            /** @description The icon's path. */
            src: string;
            /** @description Always `image/png`. */
            type: string;
        };
        /** @description What `GET /api/v1/me` answers. */
        Me: {
            /** @description The theme and density the user chose (docs/specs/m0/11-app-shell.md B21, Q6). */
            appearance: components["schemas"]["UserAppearance"];
            /**
             * @description The organization's default appearance: the theme the user's pages take while
             *     `appearance.theme` is `null`.
             */
            default_appearance: components["schemas"]["ThemeChoice"];
            /** @description What the signed-in user may read of the organization's `embedding` setting. */
            embedding: components["schemas"]["MeEmbedding"];
            /** @description The keys of the catalog features the organization's plan includes, sorted. */
            features: string[];
            /**
             * @description Each scope where the caller holds at least one effective permission, with those
             *     permissions sorted: the organization first, then accounts by id. A permission held at a
             *     scope also takes effect in every account inside it, which are not listed again.
             */
            grants: components["schemas"]["MeGrant"][];
            organization: components["schemas"]["Organization"];
            user: components["schemas"]["MeUser"];
        };
        /** @description The part of the organization's `embedding` setting every signed-in user reads. */
        MeEmbedding: {
            /**
             * @description The origins a deep link's return URL may point at for a "Back to" link to show, each
             *     `https://host[:port]` in lowercase; empty unless set.
             */
            return_origins: string[];
        };
        /** @description The permissions the caller holds at one scope. */
        MeGrant: {
            /** @description Catalog keys, sorted. */
            permissions: string[];
            scope: components["schemas"]["GrantScope"];
        };
        /** @description The signed-in user. */
        MeUser: {
            display_name: string;
            email: string | null;
            /** @description `""` when the identity provider sent none. */
            family_name: string;
            /** @description `""` when the identity provider sent none. */
            given_name: string;
            /** Format: uuid */
            id: string;
            /** @description `en` in M0. */
            locale: string;
            source: components["schemas"]["UserSource"];
            status: components["schemas"]["UserStatus"];
            /** @description An IANA time zone. */
            tz: string;
        };
        /** @description A module of a course, with its items in order. */
        Module: {
            /** @description Whether all or one of the module's requirements complete it. */
            completion_rule: components["schemas"]["CompletionRule"];
            /**
             * Format: int32
             * @description How many months a completion of the module stays valid; null for ever. A change applies
             *     to completions recorded afterwards.
             */
            completion_valid_months: number | null;
            /** Format: uuid */
            course_id: string;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /** Format: uuid */
            id: string;
            items: components["schemas"]["Item"][];
            lock: null | components["schemas"]["Lock"];
            /**
             * Format: int32
             * @description 0-based, dense within the course.
             */
            position: number;
            /** @description The modules of the course this one waits for, in course order. */
            prerequisite_ids: string[];
            progress?: null | components["schemas"]["ModuleProgress"];
            published: boolean;
            /** @description Whether the module's items go in order. */
            require_sequential: boolean;
            title: string;
            /**
             * Format: date-time
             * @description RFC 3339, UTC: the module stays locked to learners before this time.
             */
            unlock_at: string | null;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            updated_at: string;
        };
        /** @description The `data` of `module.completed` (B11). */
        ModuleCompletedData: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            completed_at: string;
            /** Format: uuid */
            course_id: string;
            /** Format: uuid */
            module_id: string;
            /** Format: uuid */
            user_id: string;
        };
        /** @description `module.completed`, schema version 1: a learner completed a module. */
        ModuleCompletedEvent: {
            data: components["schemas"]["ModuleCompletedData"];
            /**
             * Format: date-time
             * @description RFC 3339, UTC: when the change happened.
             */
            timestamp: string;
            type: components["schemas"]["ModuleCompletedType"];
        };
        /**
         * @description The type of a `module.completed` event.
         * @enum {string}
         */
        ModuleCompletedType: "module.completed";
        /** @description A course's modules, in order. */
        ModuleList: {
            modules: components["schemas"]["Module"][];
        };
        /** @description Every module id of the course, in the new order. */
        ModuleOrder: {
            module_ids: string[];
        };
        /** @description A change to a module; an omitted member is left as it is. */
        ModulePatch: {
            completion_rule?: components["schemas"]["CompletionRule"];
            /**
             * Format: int64
             * @description How many months a completion of the module stays valid, 1 to 120; `null` for ever.
             *     Completions recorded before the change keep their expiry date.
             */
            completion_valid_months?: number | null;
            /**
             * @description The whole new set of modules of the same course this one waits for: not itself, each
             *     once, at most 20, and closing no cycle.
             */
            prerequisite_ids?: string[];
            /** @description Whether learners see the module. */
            published?: boolean;
            /** @description Whether the module's items go in order. */
            require_sequential?: boolean;
            /** @description Trimmed; 1 to 200 characters. */
            title?: string;
            /**
             * Format: date-time
             * @description RFC 3339 with an offset, from 1970 up to 2200; `null` clears it.
             */
            unlock_at?: string | null;
        };
        /**
         * @description A learner's roll-up for one module: its published items with a requirement, and how many of
         *     them the learner completed.
         */
        ModuleProgress: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC: when the learner completed the module; null until then. Never cleared.
             */
            completed_at: string | null;
            /** Format: int32 */
            completed_count: number;
            /**
             * Format: date-time
             * @description RFC 3339, UTC: when that completion stops being valid; null while the module is not
             *     complete or when its completion does not expire.
             */
            expires_at: string | null;
            /** Format: uuid */
            module_id: string;
            /** Format: int32 */
            required_count: number;
        };
        /** @description One of the signed-in user's identities. */
        MyIdentity: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /** Format: uuid */
            id: string;
            /** @description The key of the identity provider. */
            provider_key: string;
            /**
             * @description The provider's label, such as "Sign in with Example"; `null` when the provider's
             *     configuration has none.
             */
            provider_label: string | null;
            /** @description The provider's id for you, exactly as the provider sends it. */
            subject: string;
        };
        /** @description One of the signed-in user's passkeys. The credential itself never leaves the server. */
        MyPasskey: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /** Format: uuid */
            id: string;
            /**
             * Format: date-time
             * @description The last sign-in with it, RFC 3339, UTC; `null` before the first.
             */
            last_used_at: string | null;
            name: string;
        };
        /** @description A new name for a passkey. */
        MyPasskeyPatch: {
            /** @description Trimmed, 1 to 200 characters. */
            name: string;
        };
        /** @description The creation options for the browser. */
        MyPasskeyStarted: {
            /**
             * @description Pass `publicKey` to `navigator.credentials.create`, then send the result to `POST
             *     /api/v1/me/passkeys` in this browser within 10 minutes.
             */
            options: components["schemas"]["PasskeyCreationOptions"];
        };
        /** @description An account to create. */
        NewAccount: {
            /** @description Trimmed; 1 to 200 characters. */
            name: string;
            /**
             * Format: uuid
             * @description The parent account; you must hold `account.manage` at a scope containing it.
             */
            parent_id: string;
        };
        /** @description A course to create, as a draft. */
        NewCourse: {
            /**
             * Format: uuid
             * @description The account the course belongs to; you must hold `course.create` at a scope containing
             *     it.
             */
            account_id: string;
            /** @description Trimmed; 1 to 50 characters; unique in the organization, ignoring ASCII case. */
            code: string;
            /** @description Trimmed; 1 to 200 characters. */
            title: string;
        };
        /** @description An enrollment to create, `active`. */
        NewEnrollmentBody: {
            /**
             * @description The learners the enrollment observes: required, 1 to 100 distinct learners of the course
             *     (users holding an enrollment there whose role has `course.participate`), when the role
             *     holds `progress.view_observed`, and refused otherwise. Naming them needs `progress.view`
             *     at the course.
             */
            observed_user_ids?: string[];
            /**
             * Format: uuid
             * @description A built-in role other than `manager`, or a custom role of the organization.
             */
            role_id: string;
            /** Format: uuid */
            user_id: string;
        };
        /** @description An identity to link to a user, so they can sign in with it the first time. */
        NewIdentity: {
            /** @description The key of one of the organization's identity providers. */
            provider_key: string;
            /** @description The provider's id for the person; stored exactly as given, not empty. */
            subject: string;
        };
        /** @description An item to create, unpublished. */
        NewItem: {
            /** @description `header` and `external_link` can be created; the other kinds arrive with their specs. */
            kind: components["schemas"]["ItemKind"];
            /**
             * Format: int64
             * @description 0 to the module's item count; last when omitted.
             */
            position?: number | null;
            /**
             * Format: uuid
             * @description What the item points at, for a kind that points at a row.
             */
            ref_id?: string | null;
            /** @description Trimmed; 1 to 200 characters. */
            title: string;
            /** @description For `external_link` only: an absolute `https` or `http` URL of at most 2,048 characters. */
            url?: string | null;
        };
        /** @description A module to create, unpublished and last. */
        NewModule: {
            /** @description Trimmed; 1 to 200 characters. */
            title: string;
        };
        /** @description The browser's registration response. */
        NewMyPasskey: {
            /**
             * @description The `PublicKeyCredential` `navigator.credentials.create` returned, as JSON
             *     (`PublicKeyCredential.toJSON()`).
             */
            credential: Record<string, never>;
            /** @description What you call this passkey: trimmed, 1 to 200 characters. `Passkey` when absent. */
            name?: string | null;
        };
        /** @description A page to create, with version 1 of its body. */
        NewPage: {
            /** @description Version 1's body; a document with one empty paragraph when omitted. */
            body?: components["schemas"]["PageDoc"];
            /**
             * @description Lowercase letters and digits in runs joined by single hyphens, 1 to 100 characters,
             *     unique in the course. Derived from the title when omitted.
             */
            slug?: string;
            /** @description Trimmed; 1 to 200 characters. */
            title: string;
        };
        /** @description A provider to create. */
        NewProvider: {
            /**
             * @description The kind's configuration: the members of an `OidcProviderConfig` or an
             *     `Oauth2ProviderConfig` and `client_secret`, which is stored encrypted and never answered.
             *     Members with a default may be left out.
             */
            config: {
                [key: string]: unknown;
            };
            /** @description Whether people can sign in through it; `true` when omitted. */
            enabled?: boolean;
            /**
             * @description 1 to 32 lowercase letters, digits and hyphens, starting with a letter or digit; not
             *     `local`. It is part of the redirect URI and never changes. Required without a preset;
             *     with one, the preset's key when omitted (`vatsim` for `vatsim`).
             */
            key?: string;
            /**
             * @description `oidc` or `oauth2`; anything else answers 422 `provider.kind_unsupported`. Required
             *     without a preset; with one, omitted or the preset's kind.
             */
            kind?: string;
            /**
             * @description 1 to 200 characters without control characters. Required without a preset; with one,
             *     the preset's label when omitted ("Sign in with VATSIM" for `vatsim`).
             */
            label?: string;
            /**
             * @description A preset to take the provider's endpoints, scopes and claims from, such as `vatsim`
             *     (VATSIM Connect, an `oauth2` provider). The config then needs only `client_id` and
             *     `client_secret`; any other member it names replaces the preset's whole. A name this
             *     server does not have answers 422 `provider.preset_unknown`.
             */
            preset?: string;
        };
        /** @description A custom role to create. */
        NewRole: {
            /**
             * @description A lowercase letter, then up to 62 lowercase letters, digits or underscores; not the key
             *     of a built-in role or another custom role of the organization.
             */
            key: string;
            /** @description Trimmed; 1 to 200 characters. */
            name: string;
            /**
             * @description Catalog keys (`GET /api/v1/admin/permissions`), each at most once and each held by the
             *     caller at organization scope.
             */
            permissions: string[];
        };
        /** @description An assignment to create. */
        NewRoleAssignment: {
            /**
             * Format: uuid
             * @description A built-in role other than `manager`, `student` and `observer`, or a custom role of the
             *     organization. Student and Observer are granted by enrolling the user in a course.
             */
            role_id: string;
            /**
             * @description The organization (its own id) or an account of it. `course` is not available: the
             *     course-scope grant is an enrollment.
             */
            scope: components["schemas"]["ScopeRef"];
            /** Format: uuid */
            user_id: string;
        };
        /** @description A service account to create. */
        NewServiceAccount: {
            /** @description Trimmed; 1 to 200 characters. */
            name: string;
            /**
             * @description Catalog permission keys (`GET /api/v1/admin/permissions`), each at most once and each
             *     held by the caller at some scope.
             */
            scopes: string[];
        };
        /** @description A user to create. The new user is `local` and `active`, and has no local credential. */
        NewUser: {
            /** @description Trimmed; 1 to 200 characters. */
            display_name: string;
            /** @description Trimmed; at most 254 characters; unique in the organization in any letter case. */
            email?: string | null;
            /** @description At most 200 characters; unique in the organization. */
            external_id?: string | null;
            /** @description At most 200 characters; `""` when omitted. */
            family_name?: string;
            /** @description At most 200 characters; `""` when omitted. */
            given_name?: string;
            /**
             * @description Identities to link to the new user, so they can sign in with them the first time; at
             *     most 20.
             */
            identities?: components["schemas"]["NewUserIdentity"][];
            /** @description `en`, the default and the only locale in M0. */
            locale?: string;
            /** @description An IANA time zone; the organization's `default_tz` when omitted. */
            tz?: string;
        };
        /** @description An identity to link to a new user: the provider's subject at the provider with this key. */
        NewUserIdentity: {
            provider_key: string;
            /** @description Stored exactly as given; not empty. */
            subject: string;
        };
        /**
         * @description The next step a password sign-in needs.
         * @enum {string}
         */
        NextStep: "totp" | "totp_enroll";
        /** @description A refused token or revocation request (RFC 6749 section 5.2). */
        OAuthError: {
            /** @description `invalid_request`, `invalid_client`, `invalid_grant` or `unsupported_grant_type`. */
            error: string;
            /** @description A fixed sentence for the app's developer. */
            error_description: string;
        };
        /** @description JSON Pointers into the userinfo body; a profile field without one is not mapped. */
        Oauth2ProviderClaims: {
            /** @description Attribute names to pointers. */
            attributes: {
                [key: string]: string;
            };
            email?: string | null;
            email_verified?: string | null;
            family_name?: string | null;
            given_name?: string | null;
            groups?: string | null;
            subject: string;
        };
        /** @description An `oauth2` provider's configuration (B4). */
        Oauth2ProviderConfig: {
            /** @description Whether users who sign in through the provider may also add passkeys. */
            allow_local_passkeys: boolean;
            authorize: string;
            claims: components["schemas"]["Oauth2ProviderClaims"];
            client_id: string;
            /** @description Whether sign-in sends S256 PKCE. */
            pkce: boolean;
            scopes: string[];
            token: string;
            /**
             * @description Whether, with the organization's email matching on, a sign-in whose verified email
             *     equals a user's verified email may be linked to that user.
             */
            trusted_for_email_matching: boolean;
            userinfo: string;
        };
        /** @description JSON Pointers into the merged ID token and userinfo claims. */
        OidcProviderClaims: {
            /** @description Attribute names to pointers. */
            attributes: {
                [key: string]: string;
            };
            email: string;
            email_verified: string;
            family_name: string;
            given_name: string;
            groups?: string | null;
        };
        /** @description An `oidc` provider's configuration (`05-oidc.md` B1). */
        OidcProviderConfig: {
            /** @description Whether users who sign in through the provider may also add passkeys. */
            allow_local_passkeys: boolean;
            claims: components["schemas"]["OidcProviderClaims"];
            client_id: string;
            /** @description The issuer, exactly as written; discovery is read from it. */
            issuer: string;
            /** @description Always includes `openid`. */
            scopes: string[];
            /**
             * @description Whether, with the organization's email matching on, a sign-in whose verified email
             *     equals a user's verified email may be linked to that user.
             */
            trusted_for_email_matching: boolean;
            /** @description Whether sign-in also reads the provider's userinfo endpoint. */
            userinfo: boolean;
        };
        /**
         * @description Every organization setting with its effective value: the stored one, or the setting's
         *     default when none is stored.
         */
        OrgSettings: {
            /** @description Who admission lets in; read-only here, changed through `PUT /api/v1/admin/admission`. */
            admission: components["schemas"]["AdmissionSwitches"];
            /** @description The IANA time zone a user created without one gets; `UTC` unless set. */
            default_tz: string;
            /**
             * @description Whether a sign-in through an identity the organization does not know yet may be linked
             *     to the existing user with the same email address: only through a provider whose config
             *     sets `trusted_for_email_matching`, only when that provider marked the address verified,
             *     and only to a local or just-in-time user whose stored email is recorded as verified.
             *     `false` unless set.
             */
            email_matching: boolean;
            embedding: components["schemas"]["Embedding"];
            /**
             * @description The local sign-in method switches; read-only here, changed through
             *     `PUT /api/v1/admin/local-sign-in`.
             */
            local_sign_in: components["schemas"]["LocalMethods"];
        };
        /**
         * @description A partial change to the organization settings: one optional member per setting this route
         *     writes; only the members present change. A member that is no setting, or a setting another
         *     route writes, is refused.
         */
        OrgSettingsPatch: {
            /** @description An IANA time zone, matched in any letter case and stored in the database's spelling. */
            default_tz?: string;
            /**
             * @description `true` needs `provider.manage` and every catalog permission at organization scope, as
             *     well as `organization.manage`.
             */
            email_matching?: boolean;
            /** @description Replaces the return origins. */
            embedding?: components["schemas"]["EmbeddingPatch"];
        };
        /** @description The caller's organization. */
        Organization: {
            /** Format: uuid */
            id: string;
            name: string;
            /** @description Fixed when the organization is created. */
            slug: string;
        };
        /**
         * @description A change to the organization; an omitted member is left as it is. A member sent as `null`
         *     is malformed: omit it instead.
         */
        OrganizationPatch: {
            /** @description Trimmed; 1 to 200 characters. */
            name?: string;
        };
        /** @description A top-level block. */
        PageBlock: {
            content?: components["schemas"]["PageInline"][];
            /** @enum {string} */
            type: "paragraph";
        } | {
            attrs: components["schemas"]["PageHeadingAttrs"];
            content?: components["schemas"]["PageInline"][];
            /** @enum {string} */
            type: "heading";
        } | {
            /** @description One or more. */
            content: components["schemas"]["PageListItem"][];
            /** @enum {string} */
            type: "bulletList";
        } | {
            attrs?: components["schemas"]["PageOrderedListAttrs"];
            /** @description One or more. */
            content: components["schemas"]["PageListItem"][];
            /** @enum {string} */
            type: "orderedList";
        } | {
            /** @description One or more. */
            content: components["schemas"]["PageNestedBlock"][];
            /** @enum {string} */
            type: "blockquote";
        } | {
            /** @enum {string} */
            type: "horizontalRule";
        } | {
            attrs?: components["schemas"]["PageCodeBlockAttrs"];
            content?: components["schemas"]["PageCodeText"][];
            /** @enum {string} */
            type: "codeBlock";
        } | {
            /** @description One or more rows. */
            content: components["schemas"]["PageTableRow"][];
            /** @enum {string} */
            type: "table";
        } | {
            attrs: components["schemas"]["PageImageAttrs"];
            /** @enum {string} */
            type: "image";
        } | {
            attrs: components["schemas"]["PageCalloutAttrs"];
            /** @description One or more. */
            content: components["schemas"]["PageNestedBlock"][];
            /** @enum {string} */
            type: "callout";
        } | {
            attrs: components["schemas"]["PageVideoEmbedAttrs"];
            /** @enum {string} */
            type: "videoEmbed";
        } | {
            /** @description One or more lines. */
            content: components["schemas"]["PageTranscriptLine"][];
            /** @enum {string} */
            type: "transcript";
        };
        /** @description A body to save as the page's new current version. */
        PageBodySave: {
            /**
             * Format: uuid
             * @description The version the body was edited from; it must still be the page's current version.
             */
            base_version_id: string;
            body: components["schemas"]["PageDoc"];
        };
        /** @description A callout's kind. */
        PageCalloutAttrs: {
            kind: components["schemas"]["PageCalloutKind"];
        };
        /** @enum {string} */
        PageCalloutKind: "info" | "tip" | "warning" | "danger";
        /** @description A cell's spans, each 1 to 20; 1 when omitted. */
        PageCellAttrs: {
            /** Format: int32 */
            colspan?: number;
            colwidth?: null;
            /** Format: int32 */
            rowspan?: number;
        };
        /**
         * @description A code block's language for highlighting, one of `bash`, `c`, `cpp`, `csharp`, `css`, `go`,
         *     `html`, `java`, `javascript`, `json`, `kotlin`, `markdown`, `php`, `python`, `ruby`, `rust`,
         *     `sql`, `swift`, `typescript`, `xml`, `yaml`, or `null` for none.
         */
        PageCodeBlockAttrs: {
            language?: string | null;
        };
        /** @description A code block's text: no marks. */
        PageCodeText: {
            /** @description Not empty; no control character but tab and newline. */
            text: string;
            type: components["schemas"]["PageTextKind"];
        };
        /**
         * @description A page body: a ProseMirror document as Tiptap serializes it. At most 1 MiB serialized and
         *     20,000 nodes. Any node, mark or attribute not listed here is refused.
         */
        PageDoc: {
            /** @description One or more blocks. */
            content: components["schemas"]["PageBlock"][];
            type: components["schemas"]["PageDocKind"];
        };
        /**
         * @description The root node's type.
         * @enum {string}
         */
        PageDocKind: "doc";
        /** @description A heading's level, 2 to 4; the page title is the level 1. */
        PageHeadingAttrs: {
            /** Format: int32 */
            level: number;
        };
        /** @description An image: a PNG, JPEG, GIF or WebP course file of the page's course. */
        PageImageAttrs: {
            /** @description The image's text alternative; empty only when `decorative`. No control character. */
            alt: string;
            /** Format: uuid */
            course_file_id: string;
            /** @description Whether the image is decorative; `false` when omitted. */
            decorative?: boolean;
        };
        /** @description Text or a line break inside a paragraph, a heading or a transcript line. */
        PageInline: {
            /** @description Each mark at most once. */
            marks?: components["schemas"]["PageMark"][];
            /** @description Not empty; no control character but tab and newline. */
            text: string;
            /** @enum {string} */
            type: "text";
        } | {
            /** @enum {string} */
            type: "hardBreak";
        };
        /**
         * @description A link's target: an absolute `https`, `http` or `mailto` URL, or a path on this site that
         *     starts with one `/`.
         */
        PageLinkAttrs: {
            href: string;
        };
        /** @description A list item: a paragraph first, then any nested blocks. */
        PageListItem: {
            content: components["schemas"]["PageNestedBlock"][];
            type: components["schemas"]["PageListItemKind"];
        };
        /** @enum {string} */
        PageListItemKind: "listItem";
        /** @description A mark on text. */
        PageMark: {
            /** @enum {string} */
            type: "bold";
        } | {
            /** @enum {string} */
            type: "italic";
        } | {
            /** @enum {string} */
            type: "underline";
        } | {
            /** @enum {string} */
            type: "strike";
        } | {
            /** @enum {string} */
            type: "code";
        } | {
            attrs: components["schemas"]["PageLinkAttrs"];
            /** @enum {string} */
            type: "link";
        } | {
            /** @enum {string} */
            type: "subscript";
        } | {
            /** @enum {string} */
            type: "superscript";
        };
        /** @description A block inside a blockquote, a callout, a list item or a table cell. */
        PageNestedBlock: {
            content?: components["schemas"]["PageInline"][];
            /** @enum {string} */
            type: "paragraph";
        } | {
            content: components["schemas"]["PageListItem"][];
            /** @enum {string} */
            type: "bulletList";
        } | {
            attrs?: components["schemas"]["PageOrderedListAttrs"];
            content: components["schemas"]["PageListItem"][];
            /** @enum {string} */
            type: "orderedList";
        } | {
            attrs?: components["schemas"]["PageCodeBlockAttrs"];
            content?: components["schemas"]["PageCodeText"][];
            /** @enum {string} */
            type: "codeBlock";
        };
        /** @description The number an ordered list starts at; 1 when omitted. */
        PageOrderedListAttrs: {
            /** Format: int32 */
            start?: number;
        };
        /** @description A change to a page; an omitted member is left as it is. */
        PagePatch: {
            /** @description As for a new page; unique in the course. */
            slug?: string;
            /** @description Trimmed; 1 to 200 characters. */
            title?: string;
        };
        /** @description The version a restore was started from. */
        PageRestore: {
            /**
             * Format: uuid
             * @description The page's current version as the caller last saw it.
             */
            base_version_id: string;
        };
        /** @description A page of a course with its current version. */
        PageSummary: {
            /** Format: uuid */
            course_id: string;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            current_version: components["schemas"]["PageVersionSummary"];
            /** Format: uuid */
            id: string;
            /** @description Unique in the course. */
            slug: string;
            title: string;
            /**
             * Format: date-time
             * @description RFC 3339, UTC: the last change to the title, the slug or the current version.
             */
            updated_at: string;
        };
        /** @description A data or header cell. */
        PageTableCell: {
            attrs?: components["schemas"]["PageCellAttrs"];
            /** @description One or more. */
            content: components["schemas"]["PageNestedBlock"][];
            type: components["schemas"]["PageTableCellKind"];
        };
        /** @enum {string} */
        PageTableCellKind: "tableCell" | "tableHeader";
        /** @description A table row of one or more cells. */
        PageTableRow: {
            content: components["schemas"]["PageTableCell"][];
            type: components["schemas"]["PageTableRowKind"];
        };
        /** @enum {string} */
        PageTableRowKind: "tableRow";
        /** @enum {string} */
        PageTextKind: "text";
        /** @description A line of a transcript: who speaks, and what. */
        PageTranscriptLine: {
            attrs: components["schemas"]["PageTranscriptLineAttrs"];
            content?: components["schemas"]["PageInline"][];
            type: components["schemas"]["PageTranscriptLineKind"];
        };
        /** @description A transcript line's speaker label, chosen per block: 1 to 40 characters, not blank. */
        PageTranscriptLineAttrs: {
            speaker: string;
        };
        /** @enum {string} */
        PageTranscriptLineKind: "transcriptLine";
        /** @description A version of a page with its body. */
        PageVersion: {
            /** Format: uuid */
            author_id: string;
            body: components["schemas"]["PageDoc"];
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /** Format: uuid */
            id: string;
            /** Format: int32 */
            number: number;
        };
        /** @description A version of a page, without its body. */
        PageVersionSummary: {
            /**
             * Format: uuid
             * @description The user who saved it.
             */
            author_id: string;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /** Format: uuid */
            id: string;
            /**
             * Format: int32
             * @description 1, 2, ... per page.
             */
            number: number;
        };
        /** @description A YouTube or Vimeo video by id; the server builds the player's URL. */
        PageVideoEmbedAttrs: {
            provider: components["schemas"]["PageVideoProvider"];
            /** @description The player's accessible name: 1 to 200 characters, not blank. */
            title: string;
            /** @description YouTube: 11 characters of `A-Z`, `a-z`, `0-9`, `_` and `-`. Vimeo: 1 to 20 digits. */
            video_id: string;
        };
        /** @enum {string} */
        PageVideoProvider: "youtube" | "vimeo";
        /** @description One page of a list. */
        Page_Account: {
            /** @description The items, in the list's sort order. */
            items: {
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                created_at: string;
                /**
                 * Format: int32
                 * @description 0 for the root, 1 for its children, and so on; at most 16.
                 */
                depth: number;
                has_children: boolean;
                /** Format: uuid */
                id: string;
                name: string;
                /**
                 * Format: uuid
                 * @description `null` for the root only. Shown even when the parent is not visible to the caller.
                 */
                parent_id: string | null;
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                updated_at: string;
            }[];
            /** @description Pass this as `cursor` to get the next page; `null` on the last page. */
            next_cursor: string | null;
        };
        /** @description One page of a list. */
        Page_Completion: {
            /** @description The items, in the list's sort order. */
            items: {
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                completed_at: string;
                /**
                 * Format: int32
                 * @description 1 for the user's first completion of the target, then 2, and so on.
                 */
                cycle: number;
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC: when the completion expired; null while it is valid.
                 */
                expired_at: string | null;
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC: when the completion stops being valid; null when it does not expire.
                 */
                expires_at: string | null;
                /** Format: uuid */
                id: string;
                target: components["schemas"]["CompletionTarget"];
                user: components["schemas"]["ProgressUser"];
            }[];
            /** @description Pass this as `cursor` to get the next page; `null` on the last page. */
            next_cursor: string | null;
        };
        /** @description One page of a list. */
        Page_Course: {
            /** @description The items, in the list's sort order. */
            items: {
                /** Format: uuid */
                account_id: string;
                code: string;
                /**
                 * Format: int32
                 * @description How many months a completion of the course stays valid; null for ever. A change applies
                 *     to completions recorded afterwards.
                 */
                completion_valid_months: number | null;
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                created_at: string;
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC. Shown to learners and staff; it opens and closes nothing.
                 */
                end_at: string | null;
                /** Format: uuid */
                id: string;
                /**
                 * @description The catalog keys grantable at course scope that you hold at this course, sorted: what
                 *     the app may offer you here. The server checks every request itself.
                 */
                permissions: string[];
                progress?: null | components["schemas"]["CourseProgress"];
                settings: components["schemas"]["CourseSettings"];
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC. Shown to learners and staff; it opens and closes nothing.
                 */
                start_at: string | null;
                status: components["schemas"]["CourseStatus"];
                /** @description A template is never published and takes no learners. */
                template: boolean;
                title: string;
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                updated_at: string;
            }[];
            /** @description Pass this as `cursor` to get the next page; `null` on the last page. */
            next_cursor: string | null;
        };
        /** @description One page of a list. */
        Page_Enrollment: {
            /** @description The items, in the list's sort order. */
            items: {
                /** Format: uuid */
                course_id: string;
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                created_at: string;
                /** @description The owning system's id for the enrollment; `null` for a `local` one. */
                external_id: string | null;
                /** Format: uuid */
                id: string;
                /**
                 * @description The learners this enrollment observes, sorted by id: present when its role holds
                 *     `progress.view_observed` (an Observer), absent otherwise.
                 */
                observed_user_ids?: string[];
                /** Format: uuid */
                role_id: string;
                source: components["schemas"]["EnrollmentSource"];
                status: components["schemas"]["EnrollmentStatus"];
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                updated_at: string;
                user: components["schemas"]["EnrolledUser"];
            }[];
            /** @description Pass this as `cursor` to get the next page; `null` on the last page. */
            next_cursor: string | null;
        };
        /** @description One page of a list. */
        Page_FolderEntry: {
            /** @description The items, in the list's sort order. */
            items: ({
                /** @enum {string} */
                kind: "folder";
                /** @description Its last segment. */
                name: string;
                /** @description The folder's whole path, such as `/handouts/week-1`. */
                path: string;
            } | {
                file: components["schemas"]["CourseFile"];
                /** @enum {string} */
                kind: "file";
            })[];
            /** @description Pass this as `cursor` to get the next page; `null` on the last page. */
            next_cursor: string | null;
        };
        /** @description One page of a list. */
        Page_LearnerProgress: {
            /** @description The items, in the list's sort order. */
            items: {
                course: null | components["schemas"]["CourseProgress"];
                /**
                 * @description `active` when any of the learner's enrollments in the course that make them a learner
                 *     is, otherwise `concluded`.
                 */
                enrollment_status: components["schemas"]["EnrollmentStatus"];
                modules: components["schemas"]["ModuleProgress"][];
                user: components["schemas"]["ProgressUser"];
            }[];
            /** @description Pass this as `cursor` to get the next page; `null` on the last page. */
            next_cursor: string | null;
        };
        /** @description One page of a list. */
        Page_PageSummary: {
            /** @description The items, in the list's sort order. */
            items: {
                /** Format: uuid */
                course_id: string;
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                created_at: string;
                current_version: components["schemas"]["PageVersionSummary"];
                /** Format: uuid */
                id: string;
                /** @description Unique in the course. */
                slug: string;
                title: string;
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC: the last change to the title, the slug or the current version.
                 */
                updated_at: string;
            }[];
            /** @description Pass this as `cursor` to get the next page; `null` on the last page. */
            next_cursor: string | null;
        };
        /** @description One page of a list. */
        Page_PageVersionSummary: {
            /** @description The items, in the list's sort order. */
            items: {
                /**
                 * Format: uuid
                 * @description The user who saved it.
                 */
                author_id: string;
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                created_at: string;
                /** Format: uuid */
                id: string;
                /**
                 * Format: int32
                 * @description 1, 2, ... per page.
                 */
                number: number;
            }[];
            /** @description Pass this as `cursor` to get the next page; `null` on the last page. */
            next_cursor: string | null;
        };
        /** @description One page of a list. */
        Page_Provider: {
            /** @description The items, in the list's sort order. */
            items: {
                /** @description Whether the provider has a client secret. The secret itself is never answered. */
                client_secret_set: boolean;
                /**
                 * @description The kind's configuration, without the client secret. For a preset provider, the
                 *     config sign-in uses: the preset's members under the provider's own.
                 */
                config: components["schemas"]["ProviderConfig"];
                /** @description Whether people can sign in through it. A disabled provider keeps its identities. */
                enabled: boolean;
                /** Format: uuid */
                id: string;
                /** @description The `{provider}` of `/auth/{provider}/start`; fixed once created. */
                key: string;
                kind: components["schemas"]["ProviderKind"];
                /** @description The text of the provider's sign-in button. */
                label: string;
                /**
                 * @description The preset the provider takes its other config members from, such as `vatsim`; absent
                 *     for a provider without one.
                 */
                preset?: string | null;
                /** @description The URI to register at the provider as the client's redirect URI. */
                redirect_uri: string;
                source: components["schemas"]["ProviderSource"];
            }[];
            /** @description Pass this as `cursor` to get the next page; `null` on the last page. */
            next_cursor: string | null;
        };
        /** @description One page of a list. */
        Page_Role: {
            /** @description The items, in the list's sort order. */
            items: {
                /** @description Built-in roles are the same in every organization and cannot be changed or deleted. */
                builtin: boolean;
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                created_at: string;
                /** Format: uuid */
                id: string;
                key: string;
                name: string;
                /** @description The permission keys the role grants. A built-in role's are this server's list for it. */
                permissions: string[];
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                updated_at: string;
            }[];
            /** @description Pass this as `cursor` to get the next page; `null` on the last page. */
            next_cursor: string | null;
        };
        /** @description One page of a list. */
        Page_RoleAssignment: {
            /** @description The items, in the list's sort order. */
            items: {
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                created_at: string;
                /** @description The owning system's id for the assignment; `null` for a `local` one. */
                external_id: string | null;
                /** Format: uuid */
                id: string;
                /** Format: uuid */
                role_id: string;
                scope: components["schemas"]["GrantScope"];
                source: components["schemas"]["AssignmentSource"];
                /** Format: uuid */
                user_id: string;
            }[];
            /** @description Pass this as `cursor` to get the next page; `null` on the last page. */
            next_cursor: string | null;
        };
        /** @description One page of a list. */
        Page_ServiceAccount: {
            /** @description The items, in the list's sort order. */
            items: {
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                created_at: string;
                /** Format: uuid */
                id: string;
                kind: components["schemas"]["ServiceAccountKind"];
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC: the last request the key authenticated, to the minute; `null` before the
                 *     first.
                 */
                last_used_at: string | null;
                name: string;
                /**
                 * Format: uuid
                 * @description The user whose current permissions the scopes are narrowed to.
                 */
                owner_user_id: string;
                /** @description The permission keys the key may use, where its owner holds them. */
                scopes: string[];
            }[];
            /** @description Pass this as `cursor` to get the next page; `null` on the last page. */
            next_cursor: string | null;
        };
        /** @description One page of a list. */
        Page_User: {
            /** @description The items, in the list's sort order. */
            items: {
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                created_at: string;
                display_name: string;
                email: string | null;
                /** @description The owning system's id for the user; `null` when there is none. */
                external_id: string | null;
                /** @description `""` when none was given. */
                family_name: string;
                /** @description `""` when none was given. */
                given_name: string;
                /** Format: uuid */
                id: string;
                /** @description `en` in M0. */
                locale: string;
                /**
                 * Format: uuid
                 * @description The user's manager, another user of the organization; `null` when they have none.
                 */
                manager_id: string | null;
                source: components["schemas"]["UserSource"];
                status: components["schemas"]["UserStatus"];
                /** @description An IANA time zone. */
                tz: string;
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                updated_at: string;
            }[];
            /** @description Pass this as `cursor` to get the next page; `null` on the last page. */
            next_cursor: string | null;
        };
        /** @description One page of a list. */
        Page_Video: {
            /** @description The items, in the list's sort order. */
            items: {
                /** @description By `at_s`. */
                chapters: components["schemas"]["Chapter"][];
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                created_at: string;
                /**
                 * Format: double
                 * @description The length in seconds, once a transcode has read it.
                 */
                duration_s?: number | null;
                /** @description Why the video failed (`video.not_a_video`, ...); only when `status` is `failed`. */
                error_code?: string | null;
                /** Format: uuid */
                id: string;
                renditions?: null | components["schemas"]["Renditions"];
                /** @description `upload` for an uploaded video. */
                source: string;
                /** @description `uploading`, `processing`, `ready` or `failed`. */
                status: string;
                title: string;
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                updated_at: string;
            }[];
            /** @description Pass this as `cursor` to get the next page; `null` on the last page. */
            next_cursor: string | null;
        };
        /**
         * @description `CredentialCreationOptions` as JSON: pass `publicKey` to
         *     `PublicKeyCredential.parseCreationOptionsFromJSON`, then to `navigator.credentials.create`.
         */
        PasskeyCreationOptions: {
            publicKey: components["schemas"]["PublicKeyCreationOptions"];
        };
        /** @description The browser's registration response for an enrollment link. */
        PasskeyEnrollment: {
            /**
             * @description The `PublicKeyCredential` `navigator.credentials.create` returned, as JSON
             *     (`PublicKeyCredential.toJSON()`).
             */
            credential: Record<string, never>;
            /** @description What the user calls this passkey: trimmed, 1 to 200 characters. `Passkey` when absent. */
            name?: string | null;
            /** @description The text after `#token=` in the link. */
            token: string;
        };
        /**
         * @description `CredentialRequestOptions` as JSON: pass `publicKey` to
         *     `PublicKeyCredential.parseRequestOptionsFromJSON`, then to `navigator.credentials.get`.
         */
        PasskeyRequestOptions: {
            publicKey: components["schemas"]["PublicKeyRequestOptions"];
        };
        /** @description The browser's answer to the request options. */
        PasskeySignInFinish: {
            /**
             * @description The `PublicKeyCredential` `navigator.credentials.get` returned, as JSON
             *     (`PublicKeyCredential.toJSON()`).
             */
            credential: Record<string, never>;
        };
        /** @description The address to sign in with. */
        PasskeySignInStart: {
            /** @description The account's email, in any letter case. */
            email: string;
            /** @description The path on this site to open after signing in; anything else becomes `/`. */
            return_to?: string | null;
        };
        /** @description The request options for the browser. */
        PasskeySignInStarted: {
            /**
             * @description Pass to `navigator.credentials.get`, then send the result to `POST
             *     /auth/local/passkey/finish` in this browser within 10 minutes.
             */
            options: components["schemas"]["PasskeyRequestOptions"];
        };
        /** @description The signed-in user's current password and the new one. Never logged. */
        PasswordChange: {
            current_password: string;
            /** @description 12 to 256 characters, not on the common-password list. */
            new_password: string;
        };
        /** @description The password to set through an enrollment link. Never logged. */
        PasswordEnrollment: {
            /** @description 12 to 256 characters, not on the common-password list. */
            password: string;
            /** @description The text after `#token=` in the link. */
            token: string;
        };
        /** @description An email and password to sign in with. Never logged. */
        PasswordSignIn: {
            /** @description The account's email, in any letter case. */
            email: string;
            password: string;
            /** @description The path on this site to open after signing in; anything else becomes `/`. */
            return_to?: string | null;
        };
        /**
         * @description The answer of a right password: the user is signed in, TOTP's second step is needed, or the
         *     organization requires TOTP and the user must set it up first.
         */
        PasswordSignedIn: components["schemas"]["SignedIn"] | components["schemas"]["CodeRequired"] | components["schemas"]["EnrollmentRequired"];
        /** @description One permission of the catalog. */
        PermissionInfo: {
            /** @description The scope types at which a role assignment grants it. */
            grantable: components["schemas"]["ScopeType"][];
            /** @description The key roles list, `area.action`. */
            key: string;
        };
        /** @description Which plan the organization runs on and how it got it. */
        PlanRef: {
            /** @description The plan's key; `unlimited` for a self-hosted organization. */
            key: string;
            source: components["schemas"]["PlanSource"];
        };
        /**
         * @description How the organization got its plan: a subscription (`stripe`), granted by the operator
         *     (`grant`), the non-profit plan (`nonprofit`), or the self-hosted `unlimited` plan
         *     (`selfhost`).
         * @enum {string}
         */
        PlanSource: "stripe" | "grant" | "nonprofit" | "selfhost";
        /** @description The body of every error response (RFC 9457). */
        Problem: {
            /** @description The stable error code, `area.reason`. */
            code: string;
            current_version?: null | components["schemas"]["CurrentVersion"];
            /** @description A sentence for a human reader. Clients branch on `code`, never on this text. */
            detail: string;
            /** @description On a 422, each rule the body broke. */
            errors?: components["schemas"]["FieldError"][];
            /**
             * @description On `entitlement.limit_reached`, the key of the limit the request would pass, such as
             *     `users`.
             */
            limit?: string | null;
            /**
             * Format: int64
             * @description On `entitlement.limit_reached`, the plan's value for that limit.
             */
            max?: number | null;
            /** @description On `item.locked`, every condition that keeps the item locked. */
            reasons?: components["schemas"]["LockReason"][];
            /**
             * Format: int32
             * @description The HTTP status code, repeated.
             */
            status: number;
            /** @description The HTTP status phrase. */
            title: string;
            /** @description `about:blank` until the docs site documents each code. */
            type: string;
        };
        /**
         * @description A step a learner reports.
         * @enum {string}
         */
        ProgressAction: "view" | "mark_done";
        /**
         * @description Where a learner is with an item.
         * @enum {string}
         */
        ProgressState: "started" | "completed";
        /** @description The body of a progress step. */
        ProgressStep: {
            action: components["schemas"]["ProgressAction"];
        };
        /** @description A learner as a progress list names them. */
        ProgressUser: {
            display_name: string;
            /** Format: uuid */
            id: string;
        };
        /** @description An `oidc` or `oauth2` provider of the organization. */
        Provider: {
            /** @description Whether the provider has a client secret. The secret itself is never answered. */
            client_secret_set: boolean;
            /**
             * @description The kind's configuration, without the client secret. For a preset provider, the
             *     config sign-in uses: the preset's members under the provider's own.
             */
            config: components["schemas"]["ProviderConfig"];
            /** @description Whether people can sign in through it. A disabled provider keeps its identities. */
            enabled: boolean;
            /** Format: uuid */
            id: string;
            /** @description The `{provider}` of `/auth/{provider}/start`; fixed once created. */
            key: string;
            kind: components["schemas"]["ProviderKind"];
            /** @description The text of the provider's sign-in button. */
            label: string;
            /**
             * @description The preset the provider takes its other config members from, such as `vatsim`; absent
             *     for a provider without one.
             */
            preset?: string | null;
            /** @description The URI to register at the provider as the client's redirect URI. */
            redirect_uri: string;
            source: components["schemas"]["ProviderSource"];
        };
        /** @description A provider's configuration, by its `kind`. */
        ProviderConfig: components["schemas"]["OidcProviderConfig"] | components["schemas"]["Oauth2ProviderConfig"];
        /**
         * @description A provider's protocol.
         * @enum {string}
         */
        ProviderKind: "oidc" | "oauth2";
        /** @description A change to a provider: send only what you change. A member sent as `null` is malformed. */
        ProviderPatch: {
            /**
             * @description Config members to replace, each whole; members not named keep their value, and an
             *     absent `client_secret` keeps the stored secret.
             */
            config?: {
                [key: string]: unknown;
            };
            enabled?: boolean;
            /** @description 1 to 200 characters without control characters. */
            label?: string;
        };
        /**
         * @description Who owns a provider's row.
         * @enum {string}
         */
        ProviderSource: "api" | "config";
        /** @description `PublicKeyCredentialCreationOptionsJSON`. */
        PublicKeyCreationOptions: {
            /** @description `none`: the server does not ask which device this is. */
            attestation?: string;
            attestationFormats?: string[];
            authenticatorSelection?: components["schemas"]["AuthenticatorSelection"];
            /** @description base64url, unpadded. */
            challenge: string;
            /** @description The user's registered passkeys, which the authenticator must not register again. */
            excludeCredentials?: components["schemas"]["CredentialDescriptor"][];
            extensions?: components["schemas"]["RegistrationExtensions"];
            hints?: string[];
            pubKeyCredParams: components["schemas"]["CredentialParameter"][];
            rp: components["schemas"]["RelyingPartyInfo"];
            /**
             * Format: int32
             * @description Milliseconds.
             */
            timeout?: number;
            user: components["schemas"]["UserInfo"];
        };
        /** @description `PublicKeyCredentialRequestOptionsJSON`. */
        PublicKeyRequestOptions: {
            /**
             * @description The passkeys the account may sign in with. Empty when the address has no account or
             *     the account has no passkey: the answer looks the same either way.
             */
            allowCredentials: components["schemas"]["CredentialDescriptor"][];
            /** @description base64url, unpadded. */
            challenge: string;
            /** @description The relying party ID: the instance's host name. */
            rpId: string;
            /**
             * Format: int32
             * @description Milliseconds.
             */
            timeout?: number;
            /** @description `required`: the authenticator checks a PIN or biometric. */
            userVerification: string;
        };
        /**
         * @description The reading font: the bundled serif, or the organization's UI font.
         * @enum {string}
         */
        ReadingFont: "serif" | "sans";
        /** @description `AuthenticationExtensionsClientInputs` for a registration. */
        RegistrationExtensions: {
            credProps?: boolean;
            credentialProtectionPolicy?: string;
            enforceCredentialProtectionPolicy?: boolean;
            hmacCreateSecret?: boolean;
            minPinLength?: boolean;
            uvm?: boolean;
        };
        /** @description `PublicKeyCredentialRpEntity`. */
        RelyingPartyInfo: {
            /** @description The relying party ID: the instance's host name. */
            id: string;
            /** @description The organization's name. */
            name: string;
        };
        /** @description The HLS renditions of a ready video. */
        Renditions: {
            /** @description Their heights in pixels, shortest first (360, 720, 1080, or one below 360). */
            heights: number[];
        };
        /**
         * @description What completes an item for a learner: `view` (open it) and `mark_done` (mark it done) on a
         *     page, file, external link or video; `watch` (at least `min_percent`, 1 to 100, of a video).
         *     `submit`, `score` and `mastery` are reserved for quizzes and activities.
         */
        Requirement: {
            /** @enum {string} */
            kind: "view";
        } | {
            /** @enum {string} */
            kind: "mark_done";
        } | {
            /** @enum {string} */
            kind: "watch";
            /**
             * Format: int32
             * @description 1 to 100.
             */
            min_percent: number;
        };
        /**
         * @description The kind of requirement a completion met.
         * @enum {string}
         */
        RequirementKind: "view" | "mark_done" | "watch";
        /** @description The revocation request's form, for the document; the handler reads the body itself. */
        RevokeForm: {
            /** @description The app's client id, `stepclimb-app`. */
            client_id: string;
            /** @description A refresh token or an access token. */
            token: string;
            /** @description `refresh_token` or `access_token`; the server tries both whatever it says. */
            token_type_hint?: string | null;
        };
        /** @description A built-in role or a custom role of the organization. */
        Role: {
            /** @description Built-in roles are the same in every organization and cannot be changed or deleted. */
            builtin: boolean;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /** Format: uuid */
            id: string;
            key: string;
            name: string;
            /** @description The permission keys the role grants. A built-in role's are this server's list for it. */
            permissions: string[];
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            updated_at: string;
        };
        /** @description A role granted to a user at a scope. */
        RoleAssignment: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /** @description The owning system's id for the assignment; `null` for a `local` one. */
            external_id: string | null;
            /** Format: uuid */
            id: string;
            /** Format: uuid */
            role_id: string;
            scope: components["schemas"]["GrantScope"];
            source: components["schemas"]["AssignmentSource"];
            /** Format: uuid */
            user_id: string;
        };
        /** @description A change to a custom role; an omitted member is left as it is. */
        RolePatch: {
            /** @description Trimmed; 1 to 200 characters. */
            name?: string | null;
            /**
             * @description The role's whole new list, each catalog key at most once. The caller must hold at
             *     organization scope every key it adds and every key it drops.
             */
            permissions?: string[] | null;
        };
        /** @description The webhook after a `PUT`, with its signing secret when this call created it. */
        SavedAdmissionWebhook: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            enabled: boolean;
            /**
             * @description Present only when this call created the webhook: the Standard Webhooks signing secret,
             *     `whsec_` and 32 bytes in base64. This is the only answer that carries it: give it to the
             *     receiver now. Rotate it to get a new one.
             */
            secret?: string | null;
            url: string;
        };
        /** @description The saved inputs, every choice made. */
        SavedInputs: {
            corner_style: components["schemas"]["CornerStyle"];
            default_appearance: components["schemas"]["ThemeChoice"];
            primary: string;
            reading_font: components["schemas"]["ReadingFont"];
            ui_font: components["schemas"]["UiFont"];
        };
        /** @description The scope a new assignment takes effect at. */
        ScopeRef: {
            /**
             * Format: uuid
             * @description The organization's own id for `organization`.
             */
            id: string;
            type: components["schemas"]["ScopeType"];
        };
        /**
         * @description The kind of scope a permission can take effect at.
         * @enum {string}
         */
        ScopeType: "organization" | "account" | "course";
        /** @description A service account. Its key is never part of it. */
        ServiceAccount: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /** Format: uuid */
            id: string;
            kind: components["schemas"]["ServiceAccountKind"];
            /**
             * Format: date-time
             * @description RFC 3339, UTC: the last request the key authenticated, to the minute; `null` before the
             *     first.
             */
            last_used_at: string | null;
            name: string;
            /**
             * Format: uuid
             * @description The user whose current permissions the scopes are narrowed to.
             */
            owner_user_id: string;
            /** @description The permission keys the key may use, where its owner holds them. */
            scopes: string[];
        };
        /**
         * @description The kind of a service account.
         * @enum {string}
         */
        ServiceAccountKind: "api_key";
        /**
         * @description The protocol of a sign-in method.
         * @enum {string}
         */
        SignInKind: "local" | "oidc" | "oauth2";
        /** @description One sign-in method the organization offers. */
        SignInMethod: {
            /**
             * @description `local` for local sign-in; otherwise the provider's key, the `{provider}` of
             *     `/auth/{provider}/start`.
             */
            key: string;
            /** @description What the method speaks. A preset provider is the kind it speaks (`oauth2`). */
            kind: components["schemas"]["SignInKind"];
            /**
             * @description The provider's configured label, such as "Sign in with Example"; `null` when its config
             *     holds none, and always for local sign-in, whose labels are the web app's.
             */
            label: string | null;
            /**
             * @description Local sign-in only: the local methods the organization offers. `totp` is the second step
             *     after `password` and is listed only beside it.
             */
            methods?: components["schemas"]["LocalMethod"][];
        };
        /** @description A completed sign-in. */
        SignedIn: {
            /** @description The path on this site to open next. */
            return_to: string;
        };
        /**
         * @description A theme. `system` follows the operating system's color and contrast settings.
         * @enum {string}
         */
        ThemeChoice: "system" | "light" | "dark" | "high-contrast";
        /** @description The token request's form, for the document; the handler reads the body itself. */
        TokenForm: {
            /** @description The app's client id, `stepclimb-app`. */
            client_id: string;
            /** @description With `authorization_code`: the code from the redirect URI. */
            code?: string | null;
            /** @description With `authorization_code`: the PKCE verifier, 43 to 128 characters. */
            code_verifier?: string | null;
            /** @description `authorization_code` or `refresh_token`. */
            grant_type: string;
            /** @description With `authorization_code`: the redirect URI the authorization request named. */
            redirect_uri?: string | null;
            /** @description With `refresh_token`: the current refresh token. */
            refresh_token?: string | null;
        };
        /** @description The answer to a granted token request (RFC 6749 section 5.1). */
        TokenResponse: {
            /** @description The access token: a bearer credential for `/api/v1`, valid `expires_in` seconds. */
            access_token: string;
            /**
             * Format: int64
             * @description Seconds the access token is valid: 900.
             * @example 900
             */
            expires_in: number;
            /** @description The refresh token: single use, valid 30 days, and never past 90 days from sign-in. */
            refresh_token: string;
            /**
             * @description Always `Bearer`.
             * @example Bearer
             */
            token_type: string;
        };
        /** @description A TOTP code, or a recovery code where one is accepted. Never logged. */
        TotpCode: {
            /**
             * @description The 6 digits the authenticator app shows (spaces ignored), or, to sign in or disable
             *     TOTP, one of the recovery codes (`ABCD-EFGH-IJKL-MNOP`, any letter case).
             */
            code: string;
        };
        /** @description The recovery codes of a confirmed TOTP, shown once. */
        TotpConfirmed: {
            /** @description Ten single-use codes that stand in for a TOTP code when the app is lost. */
            recovery_codes: string[];
        };
        /** @description A new TOTP secret, shown once. */
        TotpEnrollment: {
            /** @description The URI to show as a QR code for the authenticator app. */
            otpauth_uri: string;
            /** @description The secret in base32, for typing into the app instead. */
            secret: string;
        };
        /**
         * @description A UI font the instance bundles.
         * @enum {string}
         */
        UiFont: "Atkinson Hyperlegible Next" | "Roboto";
        /** @description A user of the caller's organization. */
        User: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            display_name: string;
            email: string | null;
            /** @description The owning system's id for the user; `null` when there is none. */
            external_id: string | null;
            /** @description `""` when none was given. */
            family_name: string;
            /** @description `""` when none was given. */
            given_name: string;
            /** Format: uuid */
            id: string;
            /** @description `en` in M0. */
            locale: string;
            /**
             * Format: uuid
             * @description The user's manager, another user of the organization; `null` when they have none.
             */
            manager_id: string | null;
            source: components["schemas"]["UserSource"];
            status: components["schemas"]["UserStatus"];
            /** @description An IANA time zone. */
            tz: string;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            updated_at: string;
        };
        /** @description The theme and density the signed-in user chose. */
        UserAppearance: {
            density: components["schemas"]["DensityChoice"];
            theme: null | components["schemas"]["ThemeChoice"];
        };
        /** @description `PublicKeyCredentialUserEntityJSON`. */
        UserInfo: {
            displayName: string;
            /** @description The user handle, base64url, unpadded: the user's id. */
            id: string;
            /** @description The user's email. */
            name: string;
        };
        /**
         * @description A change to a user; an omitted member is left as it is. A member sent as `null` is
         *     malformed, except `email` and `manager_id`, where `null` removes the value.
         */
        UserPatch: {
            /** @description Trimmed; 1 to 200 characters. Refused for a user an outside source owns. */
            display_name?: string;
            /**
             * @description Trimmed; at most 254 characters; `null` removes it. Refused for a user an outside source
             *     owns.
             */
            email?: string | null;
            /** @description At most 200 characters. Refused for a user an outside source owns. */
            family_name?: string;
            /** @description At most 200 characters. Refused for a user an outside source owns. */
            given_name?: string;
            /** @description `en` in M0. */
            locale?: string;
            /**
             * Format: uuid
             * @description The user's manager: an active user of the organization, not the user themself and not
             *     anyone whose chain of managers reaches the user; `null` removes it. Allowed whatever the
             *     user's source. The manager gets no access to anything of the user's.
             */
            manager_id?: string | null;
            /**
             * @description `suspended` ends every session of the user on its next request; you cannot suspend
             *     yourself.
             */
            status?: components["schemas"]["UserStatus"];
            /** @description An IANA time zone. */
            tz?: string;
        };
        /**
         * @description Where the user record came from: made by an admin (`local`), at sign-in through an identity
         *     provider (`idp`), or by the sync API, SCIM or a connector, which own it.
         * @enum {string}
         */
        UserSource: "local" | "idp" | "sync" | "scim" | "connector";
        /**
         * @description Whether a user may sign in.
         * @enum {string}
         */
        UserStatus: "active" | "suspended";
        /** @description A video of the organization. */
        Video: {
            /** @description By `at_s`. */
            chapters: components["schemas"]["Chapter"][];
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /**
             * Format: double
             * @description The length in seconds, once a transcode has read it.
             */
            duration_s?: number | null;
            /** @description Why the video failed (`video.not_a_video`, ...); only when `status` is `failed`. */
            error_code?: string | null;
            /** Format: uuid */
            id: string;
            renditions?: null | components["schemas"]["Renditions"];
            /** @description `upload` for an uploaded video. */
            source: string;
            /** @description `uploading`, `processing`, `ready` or `failed`. */
            status: string;
            title: string;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            updated_at: string;
        };
        /** @description A change to a video; an omitted member is left as it is. */
        VideoPatch: {
            /** @description 1 to 200 characters after trimming. */
            title?: string;
        };
        /** @description What a creation answers: the upload's path and the video it fills. */
        VideoUploadCreated: {
            /**
             * Format: uuid
             * @description The upload's id; its path is the `Location` header.
             */
            upload_id: string;
            video: components["schemas"]["Video"];
        };
        /** @description The organization's web app manifest. */
        WebManifest: {
            /** @description The organization's light theme `bg-canvas` color, as `#rrggbb`. */
            background_color: string;
            /** @description Always `standalone`. */
            display: string;
            /** @description Stepclimb's default icons: 192 and 512 px, and a maskable 512 px. */
            icons: components["schemas"]["ManifestIcon"][];
            /** @description The organization's name. */
            name: string;
            /** @description Always `/`. */
            scope: string;
            /** @description The organization's name cut to 12 characters, trailing spaces removed. */
            short_name: string;
            /** @description Always `/`. */
            start_url: string;
            /** @description The organization's light theme `brand` color, as `#rrggbb`. */
            theme_color: string;
        };
    };
    responses: {
        /** @description `account.not_found`: no account with this id exists in the organization, or you cannot see it. `org.not_found`: the request's organization cannot be determined. */
        AccountNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.invalid`: the change breaks a rule; `errors` lists each one (at most 20), each with a JSON Pointer to the member. `admission.rule_invalid` marks a rule that is not one of the documented forms or breaks a limit; `admission.attribute_unknown` an attribute no provider the list applies to maps; `admission.provider_unknown` a key in `rules.providers` that is not an outside provider of the organization. */
        AdmissionInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `admission.manage` at organization scope. `permission.escalation`: the caller does not hold every catalog permission at organization scope; removing the webhook lets in people it refuses. `auth.csrf_refused`: the request did not come from this site. */
        AdmissionWebhookDeleteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.invalid`: `errors` names `/url` with `admission.webhook_url_invalid` (not an absolute `https` URL of at most 2,048 characters without a fragment, white space or control character), `admission.webhook_url_refused` (the outbound URL policy refuses it: a user name or password, `http` or a loopback address without the instance's insecure-providers switch, a private network address on an instance with several organizations, or a link-local, multicast or reserved address) or `admission.webhook_url_required` (the organization has no webhook yet and the body names no URL). */
        AdmissionWebhookInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `admission.webhook_not_found`: the organization has no admission webhook. `org.not_found`: the request's organization cannot be determined. */
        AdmissionWebhookNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `admission.manage` at organization scope. */
        AdmissionWebhookReadForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `entitlement.feature_unavailable`: the organization's plan does not include `admission_webhook`. `permission.denied`: the caller does not hold `admission.manage` at organization scope. `permission.escalation`: the caller does not hold every catalog permission at organization scope; until the receiver has the new secret, every sign-in through an outside provider is refused. `auth.csrf_refused`: the request did not come from this site. */
        AdmissionWebhookRotateForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `admission.webhook_secret_changed`: another rotation replaced the secret while this one ran; its secret is the current one. Rotate again for a new one. */
        AdmissionWebhookSecretChanged: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `admission.manage` at organization scope. `entitlement.feature_unavailable`: the body creates the webhook, sets its URL or enables it, and the organization's plan does not include `admission_webhook`; a body that only disables it needs no feature. `permission.escalation`: the caller does not hold every catalog permission at organization scope; the webhook decides the sign-ins of every user, including users with permissions the caller lacks. `auth.csrf_refused`: the request did not come from this site. */
        AdmissionWebhookWriteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `admission.manage` at organization scope. `permission.escalation`: the body names a rule list, which applies to every user who signs in through an outside provider, and the caller does not hold every catalog permission at organization scope. `auth.csrf_refused`: the request did not come from this site. */
        AdmissionWriteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.session_required`: the request carries a bearer credential; only a signed-in browser session authorizes an app. `auth.csrf_refused`: the request did not come from this site. */
        AnswerForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `role_assignment.exists`: the user already holds the role at the scope. `role.unknown_builtin`: the role is a built-in role of another Stepclimb release. */
        AssignmentCreateConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `role.assign` at the scope. `permission.escalation`: the role has a catalog permission the caller does not hold at the scope, counting permissions that take no effect there. `auth.csrf_refused`: the request did not come from this site. */
        AssignmentCreateForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `user.not_found`: no user of the organization has this id. `role.not_found`: no built-in role and no custom role of the organization has this id. `account.not_found`: the scope names no account of the organization, or one you cannot see. `org.not_found`: the scope names another organization, or the request's organization cannot be determined. */
        AssignmentCreateNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `role.unknown_builtin`: the assignment's role is a built-in role of another Stepclimb release. `role_assignment.externally_managed`: an identity provider, the sync API, SCIM or a connector owns the assignment. `role_assignment.last_org_admin`: it is the organization's last organization-scope Organization admin assignment held by an active user. */
        AssignmentDeleteConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `role.assign` at the assignment's scope. `permission.escalation`: the role has a catalog permission the caller does not hold at the scope, counting permissions that take no effect there. `auth.csrf_refused`: the request did not come from this site. */
        AssignmentDeleteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `role_assignment.not_found`: no assignment of the organization has this id, or its scope is hidden from you. `org.not_found`: the request's organization cannot be determined. */
        AssignmentDeleteNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `account.not_found`: the scope filter names no account of the organization, or one you cannot see. `org.not_found`: the scope filter names another organization, or the request's organization cannot be determined. */
        AssignmentListNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.session_required`: the request carries a bearer credential; only a signed-in browser session authorizes an app. */
        AuthorizeForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.malformed`: the body, a path parameter or a query parameter does not parse or does not match the schema, or `limit` is outside 1 to 200. `pagination.invalid_cursor`: the cursor does not decode. */
        BadRequest: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `branding.invalid_input`: an input fails validation; `errors` names it (`branding.invalid_color`, `branding.too_long`, `branding.not_a_choice`). `branding.contrast_failed`: a generated theme has a failing contrast pair; `report` lists every appearance's pairs, failing pairs first. */
        BrandingInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["BrandingProblem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `branding.manage` at organization scope. `entitlement.feature_unavailable`: the organization's plan does not include `custom_branding`. `auth.csrf_refused`: the request did not come from this site. */
        BrandingWriteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.malformed`: `Upload-Offset` is missing or not a number, `Content-Length` is not a number, or the body is longer or shorter than it says. `video.chunk_too_small`: the chunk is under 1 MiB and does not complete the upload; send larger chunks. */
        ChunkBadRequest: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `video.upload_offset_mismatch`: `Upload-Offset` is not the upload's offset; ask for it with `HEAD` and resume from there. `course.archived`: the course is archived and takes no content change. */
        ChunkConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `file.length_required`: the request has no `Content-Length`; send the chunk with its length. */
        ChunkLengthRequired: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `storage.not_configured`: the server has no object storage configured. `file.uploads_busy`: the server is receiving as many uploads as it takes at once; try again shortly. `org.none`: the instance has no organization yet. */
        ChunkStorageUnavailable: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `file.body_timeout`: the body sent nothing for 30 seconds; send the chunk again from the stored offset. */
        ChunkTimeout: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `file.too_large`: the chunk is over 64 MiB, or runs past the upload's `Upload-Length`. */
        ChunkTooLarge: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.unsupported_media_type`: the chunk is not sent with `Content-Type: application/offset+octet-stream`. */
        ChunkUnsupportedMediaType: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you can see the course but do not hold `course.content_edit` at a scope containing it. `auth.csrf_refused`: the request did not come from this site. */
        ContentEditForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.code_taken`: another course of the organization has this code, ignoring ASCII case. */
        CourseCodeConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you can see the course but do not hold `course.view_all` at a scope containing it. `permission.escalation`: the destination would give you, over the copy, a course permission (one grantable at course scope) you do not hold over the source. `auth.csrf_refused`: the request did not come from this site. */
        CourseCopyForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.not_found`: no course with this id exists in the organization, or you cannot see it; a draft or a template also needs `course.view_all`. `account.not_found`: the destination `account_id` does not exist or you do not hold `course.create` at a scope containing it. `org.not_found`: the request's organization cannot be determined. */
        CourseCopyNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you hold `course.create` nowhere, or you can see the account without holding it there. `auth.csrf_refused`: the request did not come from this site. */
        CourseCreateForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `account.not_found`: no account with `account_id` exists in the organization, or you cannot see it. `org.not_found`: the request's organization cannot be determined. */
        CourseCreateNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: the course is archived and takes no content change. `file.in_use`: a `file` item or a version of a page references the course file; remove it first. */
        CourseFileDeleteConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.invalid`: `errors` names `/folder` (`file.folder_invalid`: not `/` or a path of `/`-separated segments that are not empty, `.` or `..` and hold no `\` or control character; `file.folder_too_long`: a segment over 100 characters; `file.folder_too_deep`: more than 8 segments) or `/name` (`file.name_invalid`: empty, `.`, `..`, or holding `/`, `\` or a control character; `file.name_too_long`: over 255 characters). Both are compared after NFC normalization. */
        CourseFileInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `file.not_found`: no course file with this id exists in the organization, or you cannot see its course. `org.not_found`: the request's organization cannot be determined. */
        CourseFileNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: the course is archived and takes no content change. `file.name_taken`: the folder already holds a file with this name. */
        CourseFileWriteConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `account.not_found`: no account with `account_id` exists in the organization, or you cannot see it. `org.not_found`: the request's organization cannot be determined. */
        CourseListNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you can see the course but do not hold `course.manage` at a scope containing it. `auth.csrf_refused`: the request did not come from this site. */
        CourseManageForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you can see the course but do not hold `course.manage` at a scope containing it. `permission.escalation`: the destination would give you, over the course, a course permission (one grantable at course scope) you do not hold over it now. `auth.csrf_refused`: the request did not come from this site. */
        CourseMoveForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.not_found`: no course with this id exists in the organization, or you cannot see it; a draft or a template also needs `course.view_all`. `account.not_found`: the destination `account_id` does not exist or you do not hold `course.create` at a scope containing it. `org.not_found`: the request's organization cannot be determined. */
        CourseMoveNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.not_found`: no course with this id exists in the organization, or you cannot see it; a draft or a template also needs `course.view_all`. `org.not_found`: the request's organization cannot be determined. */
        CourseNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.status_invalid`: the course cannot change from its status to this one; the allowed changes are draft to published, published to archived, archived to published and published to draft. `course.has_learners`: the course has an active enrollment whose role holds `course.participate`, so it cannot go back to draft. `course.template`: a template cannot be published. */
        CourseStatusConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `role.key_taken`: a built-in role or another custom role of the organization has the key. */
        CreateConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you hold `account.manage` nowhere, or you can see the parent without holding it there. `auth.csrf_refused`: the request did not come from this site. */
        CreateForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.csrf_refused`: an unsafe request without a bearer credential did not come from this site: `Sec-Fetch-Site` is not `same-origin`, `Origin` is sent but is not an allowed origin, or neither header is sent. */
        CsrfRefused: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `role.builtin_immutable`: built-in roles cannot be deleted. `role.in_use`: the role still has assignments; remove them first. */
        DeleteConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `entitlement.feature_unavailable`: the organization's plan does not include custom roles. `permission.denied`: the caller does not hold `role.manage` at organization scope. `auth.csrf_refused`: the request did not come from this site. */
        DeleteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `account.not_found`: the account does not exist or you cannot see its parent. `org.not_found`: the request's organization cannot be determined. */
        DeleteNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.session_required`: the request carries a bearer credential; an enrollment link is used from a browser. `auth.suspended`: the account is suspended. `auth.csrf_refused`: the request did not come from this site. */
        EnrollPasskeyForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.malformed`: the body is not `{token, credential, name?}`. `auth.enrollment_invalid`: the link is unknown, already used, replaced by a newer link, or for another organization. `auth.enrollment_expired`: the link is older than its 7-day lifetime. `auth.flow_invalid`: no registration was started in this browser for this link, or it was already finished; open the link again. `auth.flow_expired`: the registration was started more than 10 minutes ago; open the link again. `auth.passkey_invalid`: the passkey's response did not verify, or this passkey is already registered; open the link again and retry. */
        EnrollPasskeyInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.method_disabled`: passkey sign-in is switched off for this organization. `org.not_found`: the request's organization cannot be determined. */
        EnrollPasskeyNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.session_required`: the request carries a bearer credential; an enrollment link is used from a browser. `auth.suspended`: the account is suspended. `auth.csrf_refused`: the request did not come from this site. */
        EnrollPasswordForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.malformed`: the body is not `{token, password}`. `auth.enrollment_invalid`: the link is unknown, already used, replaced by a newer link, or for another organization. `auth.enrollment_expired`: the link is older than its 7-day lifetime. */
        EnrollPasswordInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `enrollment.exists`: the user already holds the role in the course. `role.unknown_builtin`: the role is a built-in role of another Stepclimb release. `course.archived`: an archived course takes no new enrollment. `course.template`: a template takes no enrollment whose role holds `course.participate`. */
        EnrollmentCreateConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you can see the course but do not hold `enrollment.manage` at a scope containing it. `permission.escalation`: the role has a catalog permission grantable at organization or account scope that you do not hold at the course, or the body names observed learners and you do not hold `progress.view` at the course. `auth.csrf_refused`: the request did not come from this site. */
        EnrollmentCreateForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.not_found`: no course with this id exists in the organization, or you cannot see it; a draft or a template also needs `course.view_all`. `user.not_found`: no user of the organization has this id, or your `user.view` does not cover them (held at the organization, or at an account where they hold a role or an active enrollment). `role.not_found`: no built-in role and no custom role of the organization has this id. `org.not_found`: the request's organization cannot be determined. */
        EnrollmentCreateNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `enrollment.has_progress`: the user has no other enrollment in the course whose role holds `course.participate` and has progress there, which deleting would erase; conclude the enrollment instead. `enrollment.externally_managed`: an identity provider, the sync API, SCIM or a connector owns the enrollment. `role.unknown_builtin`: the enrollment's role is a built-in role of another Stepclimb release. */
        EnrollmentDeleteConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `local.email_required`: the user has no email address, which local sign-in finds them by; add one first. */
        EnrollmentLinkConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.session_required`: the request carries a bearer credential; only a signed-in browser session creates enrollment links. `permission.denied`: the caller does not hold `user.manage` at organization scope. `permission.escalation`: the user holds a permission, at any scope, that the caller does not hold at organization scope. `auth.csrf_refused`: the request did not come from this site. */
        EnrollmentLinkForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.not_found`: no course with this id exists in the organization, or you cannot see it; a draft or a template also needs `course.view_all`. `org.not_found`: the request's organization cannot be determined. */
        EnrollmentListNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `enrollment.not_found`: no enrollment with this id exists in the organization, or you cannot see its course; a draft course or a template also needs `course.view_all`. `org.not_found`: the request's organization cannot be determined. */
        EnrollmentNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.session_required`: the request carries a bearer credential; an enrollment link is used from a browser. `auth.csrf_refused`: the request did not come from this site. */
        EnrollmentStartForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.malformed`: the body is not `{token}`. `auth.enrollment_invalid`: the link is unknown, already used, replaced by a newer link, or for another organization. `auth.enrollment_expired`: the link is older than its 7-day lifetime; ask an admin for a new one. */
        EnrollmentStartInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.method_disabled`: the organization offers neither passkey nor password sign-in. `org.not_found`: the request's organization cannot be determined. */
        EnrollmentStartNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `enrollment.externally_managed`: an identity provider, the sync API, SCIM or a connector owns the enrollment. `role.unknown_builtin`: the enrollment's role is a built-in role of another Stepclimb release. */
        EnrollmentWriteConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you can see the enrollment's course but do not hold `enrollment.manage` at a scope containing it. `permission.escalation`: the enrollment's role has a catalog permission grantable at organization or account scope that you do not hold at the course, or the change names observed learners or reactivates an enrollment that observes learners and you do not hold `progress.view` at the course. `auth.csrf_refused`: the request did not come from this site. */
        EnrollmentWriteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.not_found`: no course with this id exists in the organization, or you cannot see it. `org.not_found`: the request's organization cannot be determined. */
        FilesCourseNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you can see the course but do not hold `course.view_all` at a scope containing it. */
        FilesViewForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.invalid`: `errors` names `/from` or `/to` with `file.folder_invalid`, `file.folder_too_long` or `file.folder_too_deep` as for a course file's folder, `file.folder_root` (the top folder `/` cannot be renamed or be the new name), or `/to` with `file.folder_too_deep` when a moved file would be more than 8 levels deep. */
        FolderRenameInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.not_found`: no course with this id exists in the organization, or you cannot see it. `folder.not_found`: no file is in the folder `from` or below it. `org.not_found`: the request's organization cannot be determined. */
        FolderRenameNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `identity.last_sign_in_method`: the user would be left with no way to sign in. `identity.local`: the identity is the user's local account, which is not removed here. */
        IdentityDeleteConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `user.not_found`: no user of the organization has this id. `identity.not_found`: the user has no identity with this id. `org.not_found`: the request's organization cannot be determined. */
        IdentityDeleteNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `identity.taken`: this provider's subject is already linked to a user. */
        IdentityLinkConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `user.manage` at organization scope. `permission.escalation`: the user holds a permission, at any scope, that the caller does not hold at organization scope. `auth.session_required`: an API key acts on its own owner, or an app token on its own user. `auth.csrf_refused`: the request did not come from this site. */
        IdentityWriteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `internal.error`: the server failed; the cause is in its log. */
        InternalError: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: the course is archived and takes no content change. `item.limit_reached`: the module holds 200 items. */
        ItemCreateConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: the course is archived and takes no content change. `item.has_progress`: learners have progress on the item; unpublish it instead. */
        ItemDeleteConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.invalid`: `errors` names `/title` (`name.empty`, `name.too_long`, `text.contains_nul`), `/kind` with `item.kind_unavailable` (this release cannot create items of the kind yet), `/url` with `item.url_invalid` (not an absolute `https` or `http` URL of at most 2,048 characters with a host, without a user name, password, white space, control or Unicode format character), `item.url_required` (an external link without a URL) or `item.url_unavailable` (a URL on another kind), `/ref_id` with `item.ref_id_unavailable` (a reference on a kind that has none), `/position` with `item.position_out_of_range` (outside 0 to the module's item count), or, on a patch, `/requirement` with `requirement.invalid` (an unknown shape or member, `min_percent` outside 1 to 100, or a requirement the item's kind does not take, such as any on a header) or `requirement.unavailable` (`submit`, `score` or `mastery`, reserved for quizzes and activities). */
        ItemInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: the course is archived and takes no content change. `item.limit_reached`: the destination module holds 200 items. */
        ItemMoveConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `item.not_found`: no item with this id exists in the organization, or you cannot see its course; a draft course or a template also needs `course.view_all`. `module.not_found`: the destination module is not in the item's course. `org.not_found`: the request's organization cannot be determined. */
        ItemMoveNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `item.not_found`: no item with this id exists in the organization, or you cannot see its course; a draft course or a template also needs `course.view_all`. `org.not_found`: the request's organization cannot be determined. */
        ItemNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: the course is archived and takes no content change. `order.stale`: the list is not exactly the current set of ids (one is missing, extra or repeated); reload and try again. */
        ItemOrderConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `item.not_found`: no item with this id exists in the organization, or you cannot see it: its course is out of your reach, a draft course or a template, or the item or its module is unpublished, and you do not hold `course.view_all` at the course. `org.not_found`: the request's organization cannot be determined. */
        ItemReadNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: the course is archived and takes no content change. */
        ItemWriteConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you do not hold `progress.view` at the course, nor `progress.view_observed` through an active enrollment in it. */
        LearnerProgressForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.not_found`: no course with this id exists in the organization, or you cannot see it; a draft or a template also needs `course.view_all`. `org.not_found`: the request's organization cannot be determined. */
        LearnerProgressListNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.not_found`: no course with this id exists in the organization, or you cannot see it; a draft or a template also needs `course.view_all`. `progress.not_found`: the user is not a learner of the course (no enrollment there, active or concluded, whose role holds `course.participate`), or you read only the learners you observe and do not observe them. `org.not_found`: the request's organization cannot be determined. */
        LearnerProgressNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `local.last_method`: the change would leave the organization no way to sign in: no offered passkey, password or magic link (TOTP is only a second step), and no enabled outside provider. */
        LocalSignInConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `provider.manage` at organization scope. `permission.escalation`: the change affects how every user signs in, and the caller does not hold every catalog permission at organization scope. `auth.csrf_refused`: the request did not come from this site. */
        LocalSignInForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.invalid`: the change breaks a rule; `errors` lists each one. `local.magic_link_needs_email` marks `magic_link` switched on while the server cannot send email. `local.totp_required_needs_totp` marks `totp_required` on while `password` or `totp` is off. */
        LocalSignInInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you can see the account but do not hold `account.manage` at a scope containing it (for a delete, the account's parent). `auth.csrf_refused`: the request did not come from this site. */
        ManageForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.method_not_allowed`: the path does not take this method; `Allow` lists the methods it takes. */
        MethodNotAllowed: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.not_found`: no course with this id exists in the organization, or you cannot see it; a draft or a template also needs `course.view_all`. `org.not_found`: the request's organization cannot be determined. */
        ModuleCourseNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: the course is archived and takes no content change. `module.limit_reached`: the course holds 200 modules. */
        ModuleCreateConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: the course is archived and takes no content change. `module.not_empty`: the module has items; delete or move them first. */
        ModuleDeleteConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `module.not_found`: no module with this id exists in the organization, or you cannot see its course; a draft course or a template also needs `course.view_all`. `org.not_found`: the request's organization cannot be determined. */
        ModuleNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: the course is archived and takes no content change. `order.stale`: the list is not exactly the current set of ids (one is missing, extra or repeated); reload and try again. */
        ModuleOrderConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: the course is archived and takes no content change. `module.prerequisite_cycle`: the prerequisites would form a cycle in the course's modules. */
        ModulePatchConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.invalid`: `errors` names `/title` (`name.empty`, `name.too_long`, `text.contains_nul`), `/unlock_at` with `module.date_out_of_range` (before 1970 or from 2200 on), or `/prerequisite_ids` with `module.prerequisite_self` (the module itself), `module.prerequisite_duplicate` (an id named twice) or `module.prerequisites_too_many` (more than 20), or `/completion_valid_months` with `completion.validity_out_of_range` (outside 1 to 120). */
        ModulePatchInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `module.not_found`: no module with this id exists in the organization, or you cannot see its course, or a `prerequisite_ids` entry is not a module of the same course. `org.not_found`: the request's organization cannot be determined. */
        ModulePatchNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `module.not_found`: no module with this id exists in the organization, or you cannot see it: its course is out of your reach, a draft course or a template, or the module is unpublished, and you do not hold `course.view_all` at the course. `org.not_found`: the request's organization cannot be determined. */
        ModuleReadNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: the course is archived and takes no content change. */
        ModuleWriteConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `account.root_immutable`: the root account cannot be moved. `account.move_into_descendant`: the destination is the account itself or one of its descendants. `account.too_deep`: an account of the moved subtree would end more than 16 levels below the root. */
        MoveConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you can see the account's parent but do not hold `account.manage` there. `permission.escalation`: the destination would give you, over the moved account, a permission you do not hold over it now. `auth.csrf_refused`: the request did not come from this site. */
        MoveForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `account.not_found`: the account does not exist or you cannot see its parent, or the destination `parent_id` does not exist or is outside every subtree where you hold `account.manage`. `org.not_found`: the request's organization cannot be determined. */
        MoveNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `identity.last_sign_in_method`: you would be left with no way to sign in. `identity.local`: the identity is your local account, which is not removed here. `identity.externally_managed`: an identity provider, the sync API, SCIM or a connector owns your user record and would link the identity again; ask an admin. */
        MyIdentityDeleteConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.session_required`: the request carries a bearer credential; only a signed-in browser session removes your identities. `auth.csrf_refused`: the request did not come from this site. */
        MyIdentityDeleteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `identity.not_found`: you have no identity with this id. `org.not_found`: the request's organization cannot be determined. */
        MyIdentityNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.session_required`: the request carries a bearer credential; only a signed-in browser session manages passkeys. `local.passkeys_not_allowed`: you have no local account, and no identity provider you sign in with lets its users add a passkey. `auth.csrf_refused`: the request did not come from this site. */
        MyPasskeyAddForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.malformed`: the body is not `{credential, name?}`. `auth.flow_invalid`: no passkey registration was started in this browser for you, or it was already finished; start again. `auth.flow_expired`: the registration was started more than 10 minutes ago; start again. `auth.passkey_invalid`: the passkey's response did not verify, or this passkey is already registered; start again and retry. */
        MyPasskeyAddInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `local.last_method`: this passkey is your last way to sign in. Add another passkey before you delete this one. */
        MyPasskeyDeleteConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `passkey.not_found`: you have no passkey with this id. `auth.method_disabled`: passkey sign-in is switched off for this organization. `org.not_found`: the request's organization cannot be determined. */
        MyPasskeyNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.session_required`: the request carries a bearer credential; only a signed-in browser session manages passkeys. `auth.csrf_refused`: the request did not come from this site. */
        MyPasskeyWriteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.session_required`: the request carries a bearer credential; only a signed-in browser session manages passkeys. */
        MyPasskeysForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.method_disabled`: passkey sign-in is switched off for this organization. `org.not_found`: the request's organization cannot be determined. */
        MyPasskeysNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.session_required`: the request carries a bearer credential; only a signed-in browser session changes a password. `auth.csrf_refused`: the request did not come from this site. */
        MyPasswordForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.unauthenticated`, `auth.session_expired`, `auth.suspended`: no valid session. `auth.credentials_invalid`: the current password is wrong. */
        MyPasswordUnauthorized: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `local.totp_not_started`: no TOTP enrollment is waiting for a code; start one. `local.totp_enabled`: TOTP is already on. */
        MyTotpConfirmConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `local.totp_not_enabled`: TOTP is not on. */
        MyTotpDisableConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.session_required`: the request carries a bearer credential; only a signed-in browser session manages TOTP. `auth.csrf_refused`: the request did not come from this site. */
        MyTotpForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `local.password_required`: the account has no password, which TOTP follows. `local.totp_enabled`: TOTP is already on; disable it first. */
        MyTotpStartConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.unauthenticated`, `auth.session_expired`, `auth.suspended`: no valid session. `auth.totp_invalid`: the code is wrong, already used, or not the current one. */
        MyTotpUnauthorized: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `org.none`: the instance has no organization yet. */
        NoOrganization: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `organization.manage` at organization scope. `auth.csrf_refused`: the request did not come from this site. */
        OrgWriteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `org.not_found`: the request's organization cannot be determined. */
        OrganizationNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: the course is archived and takes no content change. `page.slug_taken`: the course already has a page with this slug. */
        PageCreateConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: the course is archived and takes no content change. `page.in_use`: an item references the page; remove it first. */
        PageDeleteConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.invalid`: `errors` names `/title` (`name.empty`, `name.too_long`, `text.contains_nul`), `/slug` with `page.slug_invalid` (not lowercase letters and digits in runs joined by single hyphens, 1 to 100 characters, or, with no slug given, a title from which none can be derived), or a member of `/body` with `page.body_invalid` (the first node, mark or attribute the page schema does not allow, or `/body` itself for a body over 1 MiB or 20,000 nodes) or `page.image_type` (an image's `course_file_id` names a file whose type, read from its bytes, is not PNG, JPEG, GIF or WebP). */
        PageInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `page.not_found`: no page with this id exists in the organization, or you cannot see its course; a draft course or a template also needs `course.view_all`. `org.not_found`: the request's organization cannot be determined. */
        PageNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: the course is archived and takes no content change. `page.slug_taken`: the course already has another page with this slug. */
        PagePatchConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.invalid`: `errors` names a member of `/body`, the version's body, with `page.image_type` (an image names a file whose type is not PNG, JPEG, GIF or WebP) or `page.body_invalid` (a node the page schema no longer allows). */
        PageRestoreInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: the course is archived and takes no content change. `page.version_conflict`: `base_version_id` is not the page's current version, which `current_version` names; nothing was saved. Reload, merge and save again. */
        PageSaveConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `page.not_found`: no page with this id exists in the organization, or you cannot see its course. `file.not_found`: an image of the body names no course file of the page's course. `org.not_found`: the request's organization cannot be determined. */
        PageSaveNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `page.not_found`: no page with this id exists in the organization, or you cannot see its course. `page_version.not_found`: the page has no version with this id. `file.not_found`: an image of the version names a course file that no longer exists. `org.not_found`: the request's organization cannot be determined. */
        PageVersionNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.not_found`: no course with this id exists in the organization, or you cannot see it; a draft or a template also needs `course.view_all`. `file.not_found`: an image of the body names no course file of this course. `org.not_found`: the request's organization cannot be determined. */
        PagesCourseNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you can see the course but do not hold `course.view_all` at a scope containing it. */
        PagesViewForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `account.not_found`: no account with `parent_id` exists in the organization, or you cannot see it. `org.not_found`: the request's organization cannot be determined. */
        ParentNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.malformed`: the body is not `{credential}`. `auth.flow_invalid`: no passkey sign-in was started in this browser, or it was already finished; start again. `auth.flow_expired`: the sign-in was started more than 10 minutes ago; start again. */
        PasskeySignInFinishBadRequest: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.suspended`: the account is suspended. `local.passkeys_not_allowed`: the account has no local account and no identity provider it signs in with allows passkeys, so its passkeys do not sign in. `auth.csrf_refused`: the request did not come from this site. */
        PasskeySignInFinishForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.method_disabled`: passkey sign-in is switched off for this organization. `org.not_found`: the request's organization cannot be determined. */
        PasskeySignInFinishNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.passkey_invalid`: the passkey did not sign in: it is not a passkey of the account the sign-in was started for, or its answer did not verify. Start again. */
        PasskeySignInFinishUnauthorized: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.method_disabled`: passkey sign-in is switched off for this organization. `org.not_found`: the request's organization cannot be determined. */
        PasskeySignInStartNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.method_disabled`: password sign-in is switched off for this organization. `org.not_found`: the request's organization cannot be determined. */
        PasswordNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.suspended`: the account is suspended. `auth.csrf_refused`: the request did not come from this site. */
        PasswordSignInForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.credentials_invalid`: the email and password do not match an account of this organization that has a password. */
        PasswordSignInUnauthorized: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.invalid` with `local.password_weak` at the password: it has fewer than 12 or more than 256 characters, or it is on the server's list of 10,000 common passwords. */
        PasswordWeak: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.too_large`: the body is larger than the server accepts. */
        PayloadTooLarge: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold the operation's permission at a scope containing its target. */
        PermissionDenied: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: the course is archived and takes no progress. */
        ProgressConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you are not a learner of the item's course (an active enrollment whose role holds `course.participate`). `item.locked`: the item is locked for you; `reasons` says what it waits for. `auth.csrf_refused`: the request did not come from this site. */
        ProgressForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.invalid`: `errors` names `/action` with `progress.action_invalid`: `view` on an item that is not a page, a file or an external link, or `mark_done` on an item whose requirement is not `mark_done`. */
        ProgressInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `item.not_found`: no published item with this id in a published module exists in the organization, or you cannot see its course; a draft course or a template also needs `course.view_all`. `org.not_found`: the request's organization cannot be determined. */
        ProgressNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `provider.key_taken`: another provider of the organization has the key. */
        ProviderCreateConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `provider.config_owned`: `stepclimb.toml` declares the provider; remove it there, and the server disables it. `provider.in_use`: identities still link users to the provider; disable it instead. `local.last_method`: deleting it would leave the organization no way to sign in. */
        ProviderDeleteConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `provider.manage` at organization scope. `auth.csrf_refused`: the request did not come from this site. */
        ProviderDeleteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `provider.config_invalid`: the config breaks a rule; `errors` lists each one (at most 20) with a JSON Pointer under `/config` and a `provider.*` code, `provider.attribute_in_use` for an attribute an admission rule tests that the config no longer maps. `provider.kind_unsupported`: the kind is not `oidc` or `oauth2`. `provider.preset_unknown`: this server has no such preset. `request.invalid`: `errors` names `/key` (`provider.key_invalid`) or `/label` (`provider.label_invalid`). */
        ProviderInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `provider.not_found`: the organization has no `oidc` or `oauth2` provider with this id. `org.not_found`: the request's organization cannot be determined. */
        ProviderNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `provider.config_owned`: `stepclimb.toml` declares the provider; change it there. `local.last_method`: disabling it would leave the organization no way to sign in. */
        ProviderUpdateConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `entitlement.feature_unavailable`: the organization's plan does not include the provider's kind (`oidc_providers` for `oidc`, `oauth2_providers` for `oauth2`). `permission.denied`: the caller does not hold `provider.manage` at organization scope; or sign-up is on and the write turns the provider on, or changes the anchors of an enabled one, without `admission.manage`; or the write maps an attribute an admission rule tests differently, or changes `client_id`, `scopes`, `userinfo`, the `oidc` issuer or the `oauth2` token URL while mapping one, without `admission.manage`. `permission.escalation`: the write trusts the provider for email matching, or changes the anchors or email claims of a trusted one, or changes the anchors of a provider identities link users to, and the caller does not hold every catalog permission at organization scope. The anchors are the issuer, the authorize origin, the whole token and userinfo URLs and the subject pointer. `auth.csrf_refused`: the request did not come from this site. */
        ProviderWriteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `provider.manage` at organization scope. */
        ProvidersForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.rate_limited`: too many sign-in or token requests from this client address, or too many failed password or TOTP checks for this account; `Retry-After` gives the seconds until the limit's window closes. */
        RateLimited: {
            headers: {
                /** @description Seconds to wait before trying again, at least 1. */
                "Retry-After"?: number;
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `role.not_found`: no built-in role and no custom role of the organization has this id. `org.not_found`: the request's organization cannot be determined. */
        RoleNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.not_found`: no API operation has this path. */
        RouteNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.session_required`: the request carries a bearer credential; only a signed-in browser session creates API keys. `entitlement.feature_unavailable`: the organization's plan does not include API keys. `permission.denied`: the caller does not hold `service_account.manage` at organization scope. `service_account.scope_exceeds_owner`: a scope names a permission the caller does not hold. `auth.csrf_refused`: the request did not come from this site. */
        ServiceAccountCreateForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `entitlement.feature_unavailable`: the organization's plan does not include API keys. `permission.denied`: the caller does not hold `service_account.manage` at organization scope. `auth.csrf_refused`: the request did not come from this site. */
        ServiceAccountDeleteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `service_account.not_found`: no service account of the organization has this id. `org.not_found`: the request's organization cannot be determined. */
        ServiceAccountNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `entitlement.feature_unavailable`: the organization's plan does not include API keys. `permission.denied`: the caller does not hold `service_account.manage` at organization scope. */
        ServiceAccountsForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.invalid`: the patch breaks a rule; `errors` lists each one. `settings.unknown_key` marks a key that is no organization setting; `settings.read_only` a setting this route does not change (`local_sign_in`: `PUT /api/v1/admin/local-sign-in`; `admission`: `PUT /api/v1/admin/admission`); `tz.unknown` a `default_tz` that is no IANA time zone; `settings.return_origin_invalid` an entry of `embedding.return_origins` that is not written `https://host[:port]`; `settings.return_origins_too_many` more than 20 of them. */
        SettingsInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `organization.manage` at organization scope, or the patch sets `email_matching` to `true` and the caller does not hold `provider.manage` there. `permission.escalation`: the patch sets `email_matching` to `true`, which changes how people sign in, and the caller does not hold every catalog permission at organization scope. `auth.csrf_refused`: the request did not come from this site. */
        SettingsWriteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: the course is archived and takes no content change. `video.status_invalid`: the upload is complete and its video is being processed; delete the video instead. */
        TerminateConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.malformed`: the body is not `{code}`. `auth.flow_invalid`: no password sign-in is waiting for TOTP set-up in this browser, it was already finished, or the password or TOTP changed since; sign in with the password again. `auth.flow_expired`: the password was checked more than 10 minutes ago; sign in with the password again. */
        TotpEnrollmentBadRequest: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.totp_invalid`: the code is wrong, or not the current one, for the secret the password step answered. The set-up is still waiting: try the current code again. */
        TotpEnrollmentUnauthorized: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.method_disabled`: TOTP is switched off for this organization, or password sign-in is, which TOTP follows. `org.not_found`: the request's organization cannot be determined. */
        TotpNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.malformed`: the body is not `{code}`. `auth.flow_invalid`: no password sign-in is waiting for a code in this browser, it was already finished, or the password or TOTP changed since; sign in with the password again. `auth.flow_expired`: the password was checked more than 10 minutes ago; sign in with the password again. */
        TotpSignInBadRequest: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.suspended`: the account is suspended. `auth.csrf_refused`: the request did not come from this site. */
        TotpSignInForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.totp_invalid`: the code is wrong, already used, or not the current one, and it is no unused recovery code. Sign in with the password again. */
        TotpSignInUnauthorized: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `tus.version_unsupported`: the request has no `Tus-Resumable: 1.0.0`; `Tus-Version` lists the version the server speaks. */
        TusVersionUnsupported: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.unauthenticated`: no credential, or one that names no session of the organization, or an `Authorization` header that is not one `Bearer <credential>`. `auth.session_expired`: the session passed its idle or absolute expiry. `auth.api_key_invalid`: the API key is malformed, fails its checksum, or names no service account of the organization. `auth.access_token_invalid`: the bearer credential is not an app access token this server issued for the organization, or its sign-in was revoked. `auth.access_token_expired`: the app access token is past its 15 minutes; refresh it. `auth.suspended`: the user, or the API key's owner, is suspended. */
        Unauthenticated: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.invalid`: the body parses but breaks a rule; `errors` lists each one. */
        UnprocessableEntity: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.unsupported_media_type`: the body is not JSON. */
        UnsupportedMediaType: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `role.builtin_immutable`: built-in roles cannot be changed. */
        UpdateConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.malformed`: `Content-Length` is not a number, the body is longer or shorter than it says, or a query parameter does not parse. */
        UploadBadRequest: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: the course is archived and takes no content change. `file.name_taken`: the folder already holds a file with this name. */
        UploadConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.not_found`: no course with this id exists in the organization, or you cannot see it. `org.not_found`: the request's organization cannot be determined. */
        UploadCourseNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you can see the course but do not hold `course.content_edit` at a scope containing it. `entitlement.limit_reached`: the file would take the organization past its plan's `storage` limit; `limit` and `max` say which and how much. `auth.csrf_refused`: the request did not come from this site. */
        UploadForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `file.length_required`: the request has no `Content-Length`; send the file with its length. */
        UploadLengthRequired: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `storage.not_configured`: the server has no object storage configured, so it cannot store files; the operator sets `STEPCLIMB_STORAGE_URL`. `file.uploads_busy`: the server is receiving as many uploads as it takes at once; try again shortly. `org.none`: the instance has no organization yet. */
        UploadStorageUnavailable: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `file.body_timeout`: the body sent nothing for 30 seconds; send the file again. */
        UploadTimeout: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `file.too_large`: `Content-Length` is over the largest file this server accepts (`max_file_bytes`, 100 MiB unless the operator set another). */
        UploadTooLarge: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `user.external_id_taken`: another user of the organization has this external id. `user.email_taken`: another user of the organization has this email, in any letter case. `identity.taken`: one of `identities` is already linked to a user. */
        UserCreateConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `user.manage` at organization scope. `entitlement.limit_reached`: the organization's plan allows no more users; `limit` and `max` name the limit and its value. `auth.csrf_refused`: the request did not come from this site. */
        UserCreateForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `user.not_found`: no user of the organization has this id. `org.not_found`: the request's organization cannot be determined. */
        UserNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `local.totp_not_enabled`: the user has no TOTP turned on. `local.cannot_remove_own_totp`: the user is the caller; turn your own TOTP off with a code from your account page. */
        UserTotpConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `user.manage` at organization scope. `permission.escalation`: the user holds a permission, at any scope, that the caller does not hold at organization scope. `auth.session_required`: an API key names its own owner. `auth.csrf_refused`: the request did not come from this site. */
        UserTotpForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `user.externally_managed`: an identity provider, the sync API, SCIM or a connector owns the user's names and email; change them there. `user.cannot_suspend_self`: you cannot suspend yourself. `user.email_taken`: another user of the organization has this email, in any letter case. `role_assignment.last_org_admin`: the user is the organization's last active Organization admin. `user.manager_cycle`: the manager is the user themself, or someone whose chain of managers reaches the user. `user.manager_inactive`: the manager is suspended. */
        UserUpdateConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `user.not_found`: no user of the organization has this id, or, for `manager_id`, the manager's id. `org.not_found`: the request's organization cannot be determined. */
        UserUpdateNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `user.manage` at organization scope. `permission.escalation`: the patch changes `email`, `status` or `manager_id` of a user who holds a permission the caller does not hold at organization scope. `auth.session_required`: an API key changes `email`, `status` or `manager_id` of its own owner, or an app token of its own user. `user.manager_self`: a session names `manager_id` on its own user; nobody sets or clears their own manager. `auth.csrf_refused`: the request did not come from this site. */
        UserWriteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you can see a course the video is attached to but do not hold `course.content_edit` at a scope containing any of them. `auth.csrf_refused`: the request did not come from this site. */
        VideoEditForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.invalid`: `errors` names `/title` with `name.empty` (empty after trimming), `name.too_long` (over 200 characters) or `text.contains_nul`. */
        VideoInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `video.not_found`: no video with this id exists in the organization, or you cannot see any course it is attached to. `org.not_found`: the request's organization cannot be determined. */
        VideoNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: every course through which you may edit the video is archived. */
        VideoRenameConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `video.status_invalid`: the video did not fail as `video.transcode_failed`, or its upload is no longer stored; upload it again as a new video. `video.retry_limit_reached`: the video has been retried 3 times, each charging transcode minutes; upload it again as a new video. `course.archived`: every course through which you may edit the video is archived. */
        VideoRetryConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.not_found`: no course with this id exists in the organization, or you cannot see it. `org.not_found`: the request's organization cannot be determined. */
        VideoUploadCourseNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.malformed`: `Upload-Length` is missing, not a number or 0, or `Upload-Metadata` is not comma-separated `key base64value` pairs with each key once and UTF-8 values. */
        VideoUploadCreateBadRequest: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.archived`: the course is archived and takes no content change. */
        VideoUploadCreateConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.invalid`: `errors` names `/title`, read from the metadata's `title` or else its `filename`: `name.empty` (neither is given, or it is empty after trimming), `name.too_long` (over 200 characters) or `text.contains_nul`. */
        VideoUploadCreateInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `file.too_large`: `Upload-Length` is over the largest video this server accepts (`max_video_bytes`, 5 GiB unless the operator set another). */
        VideoUploadCreateTooLarge: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you can see the upload's course but do not hold `course.content_edit` at a scope containing it. `auth.csrf_refused`: the request did not come from this site. */
        VideoUploadEditForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you can see the course but do not hold `course.content_edit` at a scope containing it. `entitlement.limit_reached`: the upload's length would take the organization past its plan's `video_storage` limit; `limit` and `max` say which and how much. `auth.csrf_refused`: the request did not come from this site. */
        VideoUploadForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `video_upload.not_found`: no video upload with this id exists in the organization, it is being deleted, or you cannot see its course. `org.not_found`: the request's organization cannot be determined. */
        VideoUploadNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `storage.not_configured`: the server has no object storage configured, so it cannot store videos; the operator sets `STEPCLIMB_STORAGE_URL`. `org.none`: the instance has no organization yet. */
        VideoUploadStorageUnavailable: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `course.not_found`: no course with this id exists in the organization, or you cannot see it. `org.not_found`: the request's organization cannot be determined. */
        VideosCourseNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you can see the course, or a course the video is attached to, but do not hold `course.view_all` at a scope containing it. */
        VideosViewForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `entitlement.feature_unavailable`: the organization's plan does not include custom roles. `permission.denied`: the caller does not hold `role.manage` at organization scope. `permission.escalation`: the caller does not hold, at organization scope, a permission the role gains or loses. `auth.csrf_refused`: the request did not come from this site. */
        WriteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
    };
    parameters: never;
    requestBodies: never;
    headers: never;
    pathItems: never;
}
export type $defs = Record<string, never>;
export interface operations {
    getDiscovery: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The organization's discovery document. `Cache-Control: no-cache`. */
            200: {
                headers: {
                    /** @description `no-cache` */
                    "Cache-Control"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Discovery"];
                };
            };
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listAccounts: {
        parameters: {
            query?: {
                /**
                 * @description List this account's children. Omitted, list the topmost accounts you can see with
                 *     `account.view`.
                 */
                parent_id?: string;
                /** @description The `next_cursor` of the previous page; omit it for the first page. */
                cursor?: string;
                /** @description How many items to return, 1 to 200; 50 when omitted. */
                limit?: number;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description One page of accounts */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Page_Account"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["ParentNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    createAccount: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["NewAccount"];
            };
        };
        responses: {
            /** @description The account is created */
            201: {
                headers: {
                    /** @description The account's path */
                    Location?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Account"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["CreateForbidden"];
            404: components["responses"]["ParentNotFound"];
            409: components["responses"]["CreateConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getAccount: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The account's id. */
                account_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The account */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Account"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["AccountNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deleteAccount: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The account's id. */
                account_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The account is deleted */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ManageForbidden"];
            404: components["responses"]["DeleteNotFound"];
            409: components["responses"]["DeleteConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateAccount: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The account's id. */
                account_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["AccountPatch"];
            };
        };
        responses: {
            /** @description The account after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Account"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ManageForbidden"];
            404: components["responses"]["AccountNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    moveAccount: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The account's id. */
                account_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["AccountMove"];
            };
        };
        responses: {
            /** @description The account after the move */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Account"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["MoveForbidden"];
            404: components["responses"]["MoveNotFound"];
            409: components["responses"]["MoveConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getAdmissionSettings: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The admission switches and rules */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AdmissionSettings"];
                };
            };
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateAdmissionSettings: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["AdmissionSettingsPatch"];
            };
        };
        responses: {
            /** @description The switches and rules after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AdmissionSettings"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["AdmissionWriteForbidden"];
            404: components["responses"]["OrganizationNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["AdmissionInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getAdmissionWebhook: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The admission webhook */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AdmissionWebhook"];
                };
            };
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["AdmissionWebhookReadForbidden"];
            404: components["responses"]["AdmissionWebhookNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateAdmissionWebhook: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["AdmissionWebhookPatch"];
            };
        };
        responses: {
            /** @description The webhook after the change, with `secret` when this call created it */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SavedAdmissionWebhook"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["AdmissionWebhookWriteForbidden"];
            404: components["responses"]["OrganizationNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["AdmissionWebhookInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deleteAdmissionWebhook: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The webhook and its secret are deleted */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["AdmissionWebhookDeleteForbidden"];
            404: components["responses"]["AdmissionWebhookNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    rotateAdmissionWebhookSecret: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The new signing secret */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AdmissionWebhookSecret"];
                };
            };
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["AdmissionWebhookRotateForbidden"];
            404: components["responses"]["AdmissionWebhookNotFound"];
            409: components["responses"]["AdmissionWebhookSecretChanged"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getBranding: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The organization's branding */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Branding"];
                };
            };
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateBranding: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["BrandingInputs"];
            };
        };
        responses: {
            /** @description The organization's branding after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Branding"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["BrandingWriteForbidden"];
            404: components["responses"]["OrganizationNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["BrandingInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getLocalSignIn: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The local sign-in switches */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["LocalSignIn"];
                };
            };
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateLocalSignIn: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["LocalSignIn"];
            };
        };
        responses: {
            /** @description The switches after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["LocalSignIn"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["LocalSignInForbidden"];
            404: components["responses"]["OrganizationNotFound"];
            409: components["responses"]["LocalSignInConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["LocalSignInInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listPermissions: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Every permission, in catalog order */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PermissionInfo"][];
                };
            };
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listProviders: {
        parameters: {
            query?: {
                /** @description The `next_cursor` of the previous page; omit it for the first page. */
                cursor?: string;
                /** @description How many items to return, 1 to 200; 50 when omitted. */
                limit?: number;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description One page of providers */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Page_Provider"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ProvidersForbidden"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    createProvider: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["NewProvider"];
            };
        };
        responses: {
            /** @description The provider is created */
            201: {
                headers: {
                    /** @description The provider's path */
                    Location?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Provider"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ProviderWriteForbidden"];
            404: components["responses"]["OrganizationNotFound"];
            409: components["responses"]["ProviderCreateConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["ProviderInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getProvider: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The provider's id. */
                provider_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The provider */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Provider"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ProvidersForbidden"];
            404: components["responses"]["ProviderNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deleteProvider: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The provider's id. */
                provider_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The provider is deleted */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ProviderDeleteForbidden"];
            404: components["responses"]["ProviderNotFound"];
            409: components["responses"]["ProviderDeleteConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateProvider: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The provider's id. */
                provider_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["ProviderPatch"];
            };
        };
        responses: {
            /** @description The provider after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Provider"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ProviderWriteForbidden"];
            404: components["responses"]["ProviderNotFound"];
            409: components["responses"]["ProviderUpdateConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["ProviderInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listRoleAssignments: {
        parameters: {
            query?: {
                /** @description Only this user's assignments. */
                user_id?: string;
                /** @description Only assignments at this exact scope; needs `scope_id`. */
                scope_type?: components["schemas"]["ScopeType"];
                /** @description The scope's id (the organization's own id for `organization`); needs `scope_type`. */
                scope_id?: string;
                /** @description The `next_cursor` of the previous page; omit it for the first page. */
                cursor?: string;
                /** @description How many items to return, 1 to 200; 50 when omitted. */
                limit?: number;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description One page of assignments */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Page_RoleAssignment"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["AssignmentListNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    createRoleAssignment: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["NewRoleAssignment"];
            };
        };
        responses: {
            /** @description The assignment is created */
            201: {
                headers: {
                    /** @description The assignment's path */
                    Location?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["RoleAssignment"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["AssignmentCreateForbidden"];
            404: components["responses"]["AssignmentCreateNotFound"];
            409: components["responses"]["AssignmentCreateConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deleteRoleAssignment: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The assignment's id. */
                assignment_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The assignment is removed */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["AssignmentDeleteForbidden"];
            404: components["responses"]["AssignmentDeleteNotFound"];
            409: components["responses"]["AssignmentDeleteConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listRoles: {
        parameters: {
            query?: {
                /** @description The `next_cursor` of the previous page; omit it for the first page. */
                cursor?: string;
                /** @description How many items to return, 1 to 200; 50 when omitted. */
                limit?: number;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description One page of roles */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Page_Role"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    createRole: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["NewRole"];
            };
        };
        responses: {
            /** @description The role is created */
            201: {
                headers: {
                    /** @description The role's path */
                    Location?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Role"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["WriteForbidden"];
            404: components["responses"]["OrganizationNotFound"];
            409: components["responses"]["CreateConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getRole: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The role's id. */
                role_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The role */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Role"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["RoleNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deleteRole: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The role's id. */
                role_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The role is deleted */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["DeleteForbidden"];
            404: components["responses"]["RoleNotFound"];
            409: components["responses"]["DeleteConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateRole: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The role's id. */
                role_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["RolePatch"];
            };
        };
        responses: {
            /** @description The changed role */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Role"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["WriteForbidden"];
            404: components["responses"]["RoleNotFound"];
            409: components["responses"]["UpdateConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listServiceAccounts: {
        parameters: {
            query?: {
                /** @description The `next_cursor` of the previous page; omit it for the first page. */
                cursor?: string;
                /** @description How many items to return, 1 to 200; 50 when omitted. */
                limit?: number;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description One page of service accounts */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Page_ServiceAccount"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ServiceAccountsForbidden"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    createServiceAccount: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["NewServiceAccount"];
            };
        };
        responses: {
            /** @description The service account is created; `key` is shown this once */
            201: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["CreatedServiceAccount"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ServiceAccountCreateForbidden"];
            404: components["responses"]["OrganizationNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deleteServiceAccount: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The service account's id. */
                service_account_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The service account and its key are deleted */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ServiceAccountDeleteForbidden"];
            404: components["responses"]["ServiceAccountNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    createEnrollmentLink: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The user's id. */
                user_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The link, shown once */
            201: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["EnrollmentLink"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["EnrollmentLinkForbidden"];
            404: components["responses"]["UserNotFound"];
            409: components["responses"]["EnrollmentLinkConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    removeUserTotp: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The user's id. */
                user_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The user's TOTP is off; its secret and recovery codes are deleted, and every session and app sign-in of the user has ended. */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["UserTotpForbidden"];
            404: components["responses"]["UserNotFound"];
            409: components["responses"]["UserTotpConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getCourseFile: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The course file's id. */
                course_file_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The course file */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["CourseFile"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["FilesViewForbidden"];
            404: components["responses"]["CourseFileNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deleteCourseFile: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The course file's id. */
                course_file_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The course file is deleted */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ContentEditForbidden"];
            404: components["responses"]["CourseFileNotFound"];
            409: components["responses"]["CourseFileDeleteConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateCourseFile: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The course file's id. */
                course_file_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["CourseFilePatch"];
            };
        };
        responses: {
            /** @description The course file after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["CourseFile"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ContentEditForbidden"];
            404: components["responses"]["CourseFileNotFound"];
            409: components["responses"]["CourseFileWriteConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["CourseFileInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listCourses: {
        parameters: {
            query?: {
                /** @description Only courses in this account's subtree. */
                account_id?: string;
                status?: components["schemas"]["CourseStatus"];
                template?: boolean;
                /** @description The `next_cursor` of the previous page; omit it for the first page. */
                cursor?: string;
                /** @description How many items to return, 1 to 200; 50 when omitted. */
                limit?: number;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description One page of courses */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Page_Course"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["CourseListNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    createCourse: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["NewCourse"];
            };
        };
        responses: {
            /** @description The course is created */
            201: {
                headers: {
                    /** @description The course's path */
                    Location?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Course"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["CourseCreateForbidden"];
            404: components["responses"]["CourseCreateNotFound"];
            409: components["responses"]["CourseCodeConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getCourse: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The course's id. */
                course_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The course */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Course"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["CourseNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateCourse: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The course's id. */
                course_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["CoursePatch"];
            };
        };
        responses: {
            /** @description The course after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Course"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["CourseManageForbidden"];
            404: components["responses"]["CourseNotFound"];
            409: components["responses"]["CourseCodeConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listCourseCompletions: {
        parameters: {
            query?: {
                /** @description Only this user's completions. */
                user_id?: string;
                /** @description Only completions of courses or only of modules. */
                target_type?: components["schemas"]["CompletionTargetType"];
                /** @description The `next_cursor` of the previous page; omit it for the first page. */
                cursor?: string;
                /** @description How many items to return, 1 to 200; 50 when omitted. */
                limit?: number;
            };
            header?: never;
            path: {
                /** @description The course's id. */
                course_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description One page of completions */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Page_Completion"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["LearnerProgressForbidden"];
            404: components["responses"]["LearnerProgressListNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    copyCourse: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The source course's id. */
                course_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["CourseCopy"];
            };
        };
        responses: {
            /** @description The copy is created */
            201: {
                headers: {
                    /** @description The copy's path */
                    Location?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Course"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["CourseCopyForbidden"];
            404: components["responses"]["CourseCopyNotFound"];
            409: components["responses"]["CourseCodeConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listEnrollments: {
        parameters: {
            query?: {
                status?: components["schemas"]["EnrollmentStatus"];
                role_id?: string;
                user_id?: string;
                /** @description The `next_cursor` of the previous page; omit it for the first page. */
                cursor?: string;
                /** @description How many items to return, 1 to 200; 50 when omitted. */
                limit?: number;
            };
            header?: never;
            path: {
                /** @description The course's id. */
                course_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description One page of enrollments */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Page_Enrollment"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["EnrollmentListNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    createEnrollment: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The course's id. */
                course_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["NewEnrollmentBody"];
            };
        };
        responses: {
            /** @description The enrollment is created */
            201: {
                headers: {
                    /** @description The enrollment's path */
                    Location?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Enrollment"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["EnrollmentCreateForbidden"];
            404: components["responses"]["EnrollmentCreateNotFound"];
            409: components["responses"]["EnrollmentCreateConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listCourseFiles: {
        parameters: {
            query?: {
                /** @description The folder, such as `/handouts`; `/`, the course's top folder, when omitted. */
                folder?: string;
                /** @description The `next_cursor` of the previous page; omit it for the first page. */
                cursor?: string;
                /** @description How many items to return, 1 to 200; 50 when omitted. */
                limit?: number;
            };
            header?: never;
            path: {
                /** @description The course's id. */
                course_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description One page of the folder's entries */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Page_FolderEntry"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["FilesViewForbidden"];
            404: components["responses"]["FilesCourseNotFound"];
            422: components["responses"]["CourseFileInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    uploadCourseFile: {
        parameters: {
            query: {
                /** @description The folder, such as `/handouts`; `/`, the course's top folder, when omitted. */
                folder?: string;
                /** @description The file's name in the folder. */
                name: string;
            };
            header?: never;
            path: {
                /** @description The course's id. */
                course_id: string;
            };
            cookie?: never;
        };
        /** @description The file's bytes, with `Content-Length`. */
        requestBody: {
            content: {
                "application/octet-stream": number[];
            };
        };
        responses: {
            /** @description The file is stored and placed in the folder */
            201: {
                headers: {
                    /** @description The course file's path */
                    Location?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["CourseFile"];
                };
            };
            400: components["responses"]["UploadBadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["UploadForbidden"];
            404: components["responses"]["UploadCourseNotFound"];
            408: components["responses"]["UploadTimeout"];
            409: components["responses"]["UploadConflict"];
            411: components["responses"]["UploadLengthRequired"];
            413: components["responses"]["UploadTooLarge"];
            422: components["responses"]["CourseFileInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["UploadStorageUnavailable"];
        };
    };
    renameCourseFolder: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The course's id. */
                course_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["FolderRename"];
            };
        };
        responses: {
            /** @description How many course files moved */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["FolderRenamed"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ContentEditForbidden"];
            404: components["responses"]["FolderRenameNotFound"];
            409: components["responses"]["CourseFileWriteConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["FolderRenameInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listModules: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The course's id. */
                course_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The course's outline */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ModuleList"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["ModuleCourseNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    createModule: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The course's id. */
                course_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["NewModule"];
            };
        };
        responses: {
            /** @description The module is created */
            201: {
                headers: {
                    /** @description The module's path */
                    Location?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Module"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ContentEditForbidden"];
            404: components["responses"]["ModuleCourseNotFound"];
            409: components["responses"]["ModuleCreateConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    reorderModules: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The course's id. */
                course_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["ModuleOrder"];
            };
        };
        responses: {
            /** @description The course's modules in their new order */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ModuleList"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ContentEditForbidden"];
            404: components["responses"]["ModuleCourseNotFound"];
            409: components["responses"]["ModuleOrderConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    moveCourse: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The course's id. */
                course_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["CourseMove"];
            };
        };
        responses: {
            /** @description The course after the move */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Course"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["CourseMoveForbidden"];
            404: components["responses"]["CourseMoveNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listPages: {
        parameters: {
            query?: {
                /** @description The `next_cursor` of the previous page; omit it for the first page. */
                cursor?: string;
                /** @description How many items to return, 1 to 200; 50 when omitted. */
                limit?: number;
            };
            header?: never;
            path: {
                /** @description The course's id. */
                course_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description One page of the course's pages */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Page_PageSummary"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PagesViewForbidden"];
            404: components["responses"]["ModuleCourseNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    createPage: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The course's id. */
                course_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["NewPage"];
            };
        };
        responses: {
            /** @description The page is created */
            201: {
                headers: {
                    /** @description The page's path */
                    Location?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PageSummary"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ContentEditForbidden"];
            404: components["responses"]["PagesCourseNotFound"];
            409: components["responses"]["PageCreateConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["PageInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listCourseProgress: {
        parameters: {
            query?: {
                /** @description Only learners whose enrollment status is this. */
                status?: components["schemas"]["EnrollmentStatus"];
                /** @description The `next_cursor` of the previous page; omit it for the first page. */
                cursor?: string;
                /** @description How many items to return, 1 to 200; 50 when omitted. */
                limit?: number;
            };
            header?: never;
            path: {
                /** @description The course's id. */
                course_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description One page of learners */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Page_LearnerProgress"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["LearnerProgressForbidden"];
            404: components["responses"]["LearnerProgressListNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getLearnerProgress: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The course's id. */
                course_id: string;
                /** @description The learner's user id. */
                user_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The learner's progress */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["LearnerProgressDetail"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["LearnerProgressForbidden"];
            404: components["responses"]["LearnerProgressNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    setCourseStatus: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The course's id. */
                course_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["CourseStatusChange"];
            };
        };
        responses: {
            /** @description The course after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Course"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["CourseManageForbidden"];
            404: components["responses"]["CourseNotFound"];
            409: components["responses"]["CourseStatusConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    createVideoUpload: {
        parameters: {
            query?: never;
            header: {
                /** @description `1.0.0`. */
                "Tus-Resumable": string;
                /** @description The video's length in bytes. */
                "Upload-Length": number;
                /** @description Comma-separated `key base64value` pairs; `title` and `filename` are read. */
                "Upload-Metadata"?: string | null;
            };
            path: {
                /** @description The course's id. */
                course_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The upload is started; send the bytes with PATCH to its Location */
            201: {
                headers: {
                    /** @description The upload's path */
                    Location?: string;
                    /** @description `1.0.0` */
                    "Tus-Resumable"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["VideoUploadCreated"];
                };
            };
            400: components["responses"]["VideoUploadCreateBadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["VideoUploadForbidden"];
            404: components["responses"]["VideoUploadCourseNotFound"];
            409: components["responses"]["VideoUploadCreateConflict"];
            412: components["responses"]["TusVersionUnsupported"];
            413: components["responses"]["VideoUploadCreateTooLarge"];
            422: components["responses"]["VideoUploadCreateInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["VideoUploadStorageUnavailable"];
        };
    };
    listCourseVideos: {
        parameters: {
            query?: {
                /** @description The `next_cursor` of the previous page; omit it for the first page. */
                cursor?: string;
                /** @description How many items to return, 1 to 200; 50 when omitted. */
                limit?: number;
            };
            header?: never;
            path: {
                /** @description The course's id. */
                course_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description One page of the course's videos */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Page_Video"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["VideosViewForbidden"];
            404: components["responses"]["VideosCourseNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deleteEnrollment: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The enrollment's id. */
                enrollment_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The enrollment is removed */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["EnrollmentWriteForbidden"];
            404: components["responses"]["EnrollmentNotFound"];
            409: components["responses"]["EnrollmentDeleteConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateEnrollment: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The enrollment's id. */
                enrollment_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["EnrollmentPatch"];
            };
        };
        responses: {
            /** @description The enrollment after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Enrollment"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["EnrollmentWriteForbidden"];
            404: components["responses"]["EnrollmentNotFound"];
            409: components["responses"]["EnrollmentWriteConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getItem: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The item's id. */
                item_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The item */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Item"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["ItemReadNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deleteItem: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The item's id. */
                item_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The item is deleted */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ContentEditForbidden"];
            404: components["responses"]["ItemNotFound"];
            409: components["responses"]["ItemDeleteConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateItem: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The item's id. */
                item_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["ItemPatch"];
            };
        };
        responses: {
            /** @description The item after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Item"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ContentEditForbidden"];
            404: components["responses"]["ItemNotFound"];
            409: components["responses"]["ItemWriteConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["ItemInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    moveItem: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The item's id. */
                item_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["ItemMove"];
            };
        };
        responses: {
            /** @description The item after the move */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Item"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ContentEditForbidden"];
            404: components["responses"]["ItemMoveNotFound"];
            409: components["responses"]["ItemMoveConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["ItemInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    recordItemProgress: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The item's id. */
                item_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["ProgressStep"];
            };
        };
        responses: {
            /** @description Your progress on the item after the step */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ItemProgress"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ProgressForbidden"];
            404: components["responses"]["ProgressNotFound"];
            409: components["responses"]["ProgressConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["ProgressInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getMe: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The signed-in user and their organization */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Me"];
                };
            };
            401: components["responses"]["Unauthenticated"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateMyAppearance: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["UserAppearance"];
            };
        };
        responses: {
            /** @description The appearance after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["UserAppearance"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["CsrfRefused"];
            404: components["responses"]["OrganizationNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listMyCompletions: {
        parameters: {
            query?: {
                /** @description Only completions of this course and its modules. */
                course_id?: string;
                /** @description The `next_cursor` of the previous page; omit it for the first page. */
                cursor?: string;
                /** @description How many items to return, 1 to 200; 50 when omitted. */
                limit?: number;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description One page of your completions */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Page_Completion"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listMyIdentities: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Your identities */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["MyIdentity"][];
                };
            };
            401: components["responses"]["Unauthenticated"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deleteMyIdentity: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The identity's id. */
                identity_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The identity is removed */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["MyIdentityDeleteForbidden"];
            404: components["responses"]["MyIdentityNotFound"];
            409: components["responses"]["MyIdentityDeleteConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listMyPasskeys: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Your passkeys */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["MyPasskey"][];
                };
            };
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["MyPasskeysForbidden"];
            404: components["responses"]["MyPasskeysNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    createMyPasskey: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["NewMyPasskey"];
            };
        };
        responses: {
            /** @description The passkey is added */
            201: {
                headers: {
                    /** @description `__Host-sc_flow` cleared */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["MyPasskey"];
                };
            };
            400: components["responses"]["MyPasskeyAddInvalid"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["MyPasskeyAddForbidden"];
            404: components["responses"]["MyPasskeysNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    startMyPasskey: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The creation options; the registration is waiting in this browser */
            200: {
                headers: {
                    /** @description The `__Host-sc_flow` cookie of the registration */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["MyPasskeyStarted"];
                };
            };
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["MyPasskeyAddForbidden"];
            404: components["responses"]["MyPasskeysNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deleteMyPasskey: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The passkey's id. */
                passkey_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The passkey is deleted */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["MyPasskeyWriteForbidden"];
            404: components["responses"]["MyPasskeyNotFound"];
            409: components["responses"]["MyPasskeyDeleteConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateMyPasskey: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The passkey's id. */
                passkey_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["MyPasskeyPatch"];
            };
        };
        responses: {
            /** @description The passkey with its new name */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["MyPasskey"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["MyPasskeyWriteForbidden"];
            404: components["responses"]["MyPasskeyNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    changeMyPassword: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["PasswordChange"];
            };
        };
        responses: {
            /** @description The password is changed and every other session of the user has ended */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["MyPasswordUnauthorized"];
            403: components["responses"]["MyPasswordForbidden"];
            404: components["responses"]["PasswordNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["PasswordWeak"];
            429: components["responses"]["RateLimited"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    startMyTotp: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description A new unconfirmed secret, shown once; it replaces an unconfirmed one */
            200: {
                headers: {
                    /** @description `no-store` */
                    "Cache-Control"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["TotpEnrollment"];
                };
            };
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["MyTotpForbidden"];
            404: components["responses"]["TotpNotFound"];
            409: components["responses"]["MyTotpStartConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    confirmMyTotp: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["TotpCode"];
            };
        };
        responses: {
            /** @description TOTP is on; the recovery codes are shown once */
            200: {
                headers: {
                    /** @description `no-store` */
                    "Cache-Control"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["TotpConfirmed"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["MyTotpUnauthorized"];
            403: components["responses"]["MyTotpForbidden"];
            404: components["responses"]["TotpNotFound"];
            409: components["responses"]["MyTotpConfirmConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            429: components["responses"]["RateLimited"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    disableMyTotp: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["TotpCode"];
            };
        };
        responses: {
            /** @description TOTP is off; its secret and recovery codes are deleted */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["MyTotpUnauthorized"];
            403: components["responses"]["MyTotpForbidden"];
            404: components["responses"]["TotpNotFound"];
            409: components["responses"]["MyTotpDisableConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            429: components["responses"]["RateLimited"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getModule: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The module's id. */
                module_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The module */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Module"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["ModuleReadNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deleteModule: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The module's id. */
                module_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The module is deleted */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ContentEditForbidden"];
            404: components["responses"]["ModuleNotFound"];
            409: components["responses"]["ModuleDeleteConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateModule: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The module's id. */
                module_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["ModulePatch"];
            };
        };
        responses: {
            /** @description The module after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Module"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ContentEditForbidden"];
            404: components["responses"]["ModulePatchNotFound"];
            409: components["responses"]["ModulePatchConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["ModulePatchInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    createItem: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The module's id. */
                module_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["NewItem"];
            };
        };
        responses: {
            /** @description The item is created */
            201: {
                headers: {
                    /** @description The item's path */
                    Location?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Item"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ContentEditForbidden"];
            404: components["responses"]["ModuleNotFound"];
            409: components["responses"]["ItemCreateConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["ItemInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    reorderItems: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The module's id. */
                module_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["ItemOrder"];
            };
        };
        responses: {
            /** @description The module with its items in their new order */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Module"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ContentEditForbidden"];
            404: components["responses"]["ModuleNotFound"];
            409: components["responses"]["ItemOrderConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getOrganization: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The caller's organization */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Organization"];
                };
            };
            401: components["responses"]["Unauthenticated"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateOrganization: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["OrganizationPatch"];
            };
        };
        responses: {
            /** @description The organization after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Organization"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["OrgWriteForbidden"];
            404: components["responses"]["OrganizationNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getOrganizationEntitlements: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The plan and what it allows */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Entitlements"];
                };
            };
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getOrganizationSettings: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Every setting with its effective value */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["OrgSettings"];
                };
            };
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateOrganizationSettings: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["OrgSettingsPatch"];
            };
        };
        responses: {
            /** @description Every setting after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["OrgSettings"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["SettingsWriteForbidden"];
            404: components["responses"]["OrganizationNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["SettingsInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deletePage: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The page's id. */
                page_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The page is deleted */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ContentEditForbidden"];
            404: components["responses"]["PageNotFound"];
            409: components["responses"]["PageDeleteConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updatePage: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The page's id. */
                page_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["PagePatch"];
            };
        };
        responses: {
            /** @description The page after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PageSummary"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ContentEditForbidden"];
            404: components["responses"]["PageNotFound"];
            409: components["responses"]["PagePatchConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["PageInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    savePageBody: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The page's id. */
                page_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["PageBodySave"];
            };
        };
        responses: {
            /** @description The page with its current version */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PageSummary"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ContentEditForbidden"];
            404: components["responses"]["PageSaveNotFound"];
            409: components["responses"]["PageSaveConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["PageInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listPageVersions: {
        parameters: {
            query?: {
                /** @description The `next_cursor` of the previous page; omit it for the first page. */
                cursor?: string;
                /** @description How many items to return, 1 to 200; 50 when omitted. */
                limit?: number;
            };
            header?: never;
            path: {
                /** @description The page's id. */
                page_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description One page of the versions */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Page_PageVersionSummary"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PagesViewForbidden"];
            404: components["responses"]["PageNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getPageVersion: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The page's id. */
                page_id: string;
                /** @description The version's id. */
                version_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The version with its body */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PageVersion"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PagesViewForbidden"];
            404: components["responses"]["PageVersionNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    restorePageVersion: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The page's id. */
                page_id: string;
                /** @description The version to restore. */
                version_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["PageRestore"];
            };
        };
        responses: {
            /** @description The page with its current version */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PageSummary"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ContentEditForbidden"];
            404: components["responses"]["PageVersionNotFound"];
            409: components["responses"]["PageSaveConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["PageRestoreInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listUsers: {
        parameters: {
            query?: {
                status?: components["schemas"]["UserStatus"];
                source?: components["schemas"]["UserSource"];
                /** @description A prefix of the display name or the email, in any letter case. */
                q?: string;
                /** @description The `next_cursor` of the previous page; omit it for the first page. */
                cursor?: string;
                /** @description How many items to return, 1 to 200; 50 when omitted. */
                limit?: number;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description One page of users */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Page_User"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    createUser: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["NewUser"];
            };
        };
        responses: {
            /** @description The user is created */
            201: {
                headers: {
                    /** @description The user's path */
                    Location?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["User"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["UserCreateForbidden"];
            404: components["responses"]["OrganizationNotFound"];
            409: components["responses"]["UserCreateConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getUser: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The user's id. */
                user_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The user */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["User"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["UserNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateUser: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The user's id. */
                user_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["UserPatch"];
            };
        };
        responses: {
            /** @description The user after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["User"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["UserWriteForbidden"];
            404: components["responses"]["UserUpdateNotFound"];
            409: components["responses"]["UserUpdateConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listUserIdentities: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The user's id. */
                user_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The user's identities */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Identity"][];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["UserNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    createUserIdentity: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The user's id. */
                user_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["NewIdentity"];
            };
        };
        responses: {
            /** @description The identity is linked */
            201: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Identity"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["IdentityWriteForbidden"];
            404: components["responses"]["UserNotFound"];
            409: components["responses"]["IdentityLinkConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deleteUserIdentity: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The user's id. */
                user_id: string;
                /** @description The identity's id. */
                identity_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The identity is removed */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["IdentityWriteForbidden"];
            404: components["responses"]["IdentityDeleteNotFound"];
            409: components["responses"]["IdentityDeleteConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deleteVideoUpload: {
        parameters: {
            query?: never;
            header: {
                /** @description `1.0.0`. */
                "Tus-Resumable": string;
            };
            path: {
                /** @description The upload's id. */
                upload_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The upload and its video are deleted */
            204: {
                headers: {
                    /** @description `1.0.0` */
                    "Tus-Resumable"?: string;
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["VideoUploadEditForbidden"];
            404: components["responses"]["VideoUploadNotFound"];
            409: components["responses"]["TerminateConflict"];
            412: components["responses"]["TusVersionUnsupported"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["VideoUploadStorageUnavailable"];
        };
    };
    getVideoUploadOffset: {
        parameters: {
            query?: never;
            header: {
                /** @description `1.0.0`. */
                "Tus-Resumable": string;
            };
            path: {
                /** @description The upload's id. */
                upload_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The upload's offset and length */
            200: {
                headers: {
                    /** @description `1.0.0` */
                    "Tus-Resumable"?: string;
                    /** @description The upload's whole length */
                    "Upload-Length"?: number;
                    /** @description The bytes stored, where the next chunk starts */
                    "Upload-Offset"?: number;
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["VideoUploadEditForbidden"];
            404: components["responses"]["VideoUploadNotFound"];
            412: components["responses"]["TusVersionUnsupported"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    appendVideoUpload: {
        parameters: {
            query?: never;
            header: {
                /** @description `1.0.0`. */
                "Tus-Resumable": string;
                /** @description Where this chunk starts: the offset `HEAD` answers. */
                "Upload-Offset": number;
            };
            path: {
                /** @description The upload's id. */
                upload_id: string;
            };
            cookie?: never;
        };
        /** @description The chunk's bytes, with `Content-Length`, at most 64 MiB. */
        requestBody: {
            content: {
                "application/offset+octet-stream": number[];
            };
        };
        responses: {
            /** @description The chunk is stored */
            204: {
                headers: {
                    /** @description `1.0.0` */
                    "Tus-Resumable"?: string;
                    /** @description The bytes stored now */
                    "Upload-Offset"?: number;
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["ChunkBadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["VideoUploadEditForbidden"];
            404: components["responses"]["VideoUploadNotFound"];
            408: components["responses"]["ChunkTimeout"];
            409: components["responses"]["ChunkConflict"];
            411: components["responses"]["ChunkLengthRequired"];
            412: components["responses"]["TusVersionUnsupported"];
            413: components["responses"]["ChunkTooLarge"];
            415: components["responses"]["ChunkUnsupportedMediaType"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["ChunkStorageUnavailable"];
        };
    };
    getVideo: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The video's id. */
                video_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The video */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Video"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["VideosViewForbidden"];
            404: components["responses"]["VideoNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateVideo: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The video's id. */
                video_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["VideoPatch"];
            };
        };
        responses: {
            /** @description The video after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Video"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["VideoEditForbidden"];
            404: components["responses"]["VideoNotFound"];
            409: components["responses"]["VideoRenameConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["VideoInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    retryVideo: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The video's id. */
                video_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The video, processing again */
            202: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Video"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["VideoEditForbidden"];
            404: components["responses"]["VideoNotFound"];
            409: components["responses"]["VideoRetryConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    enrollPasskey: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["PasskeyEnrollment"];
            };
        };
        responses: {
            /** @description The passkey is registered and the user is signed in */
            200: {
                headers: {
                    /** @description The new `__Host-sc_session` cookie, and `__Host-sc_flow` cleared */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SignedIn"];
                };
            };
            400: components["responses"]["EnrollPasskeyInvalid"];
            403: components["responses"]["EnrollPasskeyForbidden"];
            404: components["responses"]["EnrollPasskeyNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            429: components["responses"]["RateLimited"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    enrollPassword: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["PasswordEnrollment"];
            };
        };
        responses: {
            /** @description The password is set and the user is signed in */
            200: {
                headers: {
                    /** @description `no-store` */
                    "Cache-Control"?: string;
                    /** @description The new `__Host-sc_session` cookie and the `__Host-sc_device` cookie */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SignedIn"];
                };
            };
            400: components["responses"]["EnrollPasswordInvalid"];
            403: components["responses"]["EnrollPasswordForbidden"];
            404: components["responses"]["PasswordNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["PasswordWeak"];
            429: components["responses"]["RateLimited"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    startEnrollment: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["EnrollmentStart"];
            };
        };
        responses: {
            /** @description The link is live */
            200: {
                headers: {
                    /** @description `no-store`: the answer holds the user's email */
                    "Cache-Control"?: string;
                    /** @description With `passkey_options`: the `__Host-sc_flow` cookie of the passkey registration */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["EnrollmentStarted"];
                };
            };
            400: components["responses"]["EnrollmentStartInvalid"];
            403: components["responses"]["EnrollmentStartForbidden"];
            404: components["responses"]["EnrollmentStartNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            429: components["responses"]["RateLimited"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    finishPasskeySignIn: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["PasskeySignInFinish"];
            };
        };
        responses: {
            /** @description The user is signed in */
            200: {
                headers: {
                    /** @description The new `__Host-sc_session` cookie, and `__Host-sc_flow` cleared */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SignedIn"];
                };
            };
            400: components["responses"]["PasskeySignInFinishBadRequest"];
            401: components["responses"]["PasskeySignInFinishUnauthorized"];
            403: components["responses"]["PasskeySignInFinishForbidden"];
            404: components["responses"]["PasskeySignInFinishNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            429: components["responses"]["RateLimited"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    startPasskeySignIn: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["PasskeySignInStart"];
            };
        };
        responses: {
            /** @description The request options, whether or not the address has an account or a passkey */
            200: {
                headers: {
                    /** @description `no-store` */
                    "Cache-Control"?: string;
                    /** @description The `__Host-sc_flow` cookie of the sign-in */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PasskeySignInStarted"];
                };
            };
            400: components["responses"]["BadRequest"];
            403: components["responses"]["CsrfRefused"];
            404: components["responses"]["PasskeySignInStartNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            429: components["responses"]["RateLimited"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    signInWithPassword: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["PasswordSignIn"];
            };
        };
        responses: {
            /** @description The user is signed in (`{return_to}`), or the account also needs a TOTP code (`{next: "totp"}`) and no session exists yet */
            200: {
                headers: {
                    /** @description `no-store` */
                    "Cache-Control"?: string;
                    /** @description Signed in: the new `__Host-sc_session` cookie and the `__Host-sc_device` cookie. A code needed: the `__Host-sc_flow` cookie of the second step */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PasswordSignedIn"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["PasswordSignInUnauthorized"];
            403: components["responses"]["PasswordSignInForbidden"];
            404: components["responses"]["PasswordNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            429: components["responses"]["RateLimited"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    signInWithTotp: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["TotpCode"];
            };
        };
        responses: {
            /** @description The user is signed in */
            200: {
                headers: {
                    /** @description `no-store` */
                    "Cache-Control"?: string;
                    /** @description The new `__Host-sc_session` cookie, the `__Host-sc_device` cookie, and `__Host-sc_flow` cleared */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SignedIn"];
                };
            };
            400: components["responses"]["TotpSignInBadRequest"];
            401: components["responses"]["TotpSignInUnauthorized"];
            403: components["responses"]["TotpSignInForbidden"];
            404: components["responses"]["TotpNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            429: components["responses"]["RateLimited"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    enrollTotpAtSignIn: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["TotpCode"];
            };
        };
        responses: {
            /** @description TOTP is on and the user is signed in; the recovery codes are shown once */
            200: {
                headers: {
                    /** @description `no-store` */
                    "Cache-Control"?: string;
                    /** @description The new `__Host-sc_session` cookie, the `__Host-sc_device` cookie, and `__Host-sc_flow` cleared */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["EnrolledSignedIn"];
                };
            };
            400: components["responses"]["TotpEnrollmentBadRequest"];
            401: components["responses"]["TotpEnrollmentUnauthorized"];
            403: components["responses"]["TotpSignInForbidden"];
            404: components["responses"]["TotpNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            429: components["responses"]["RateLimited"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    signOut: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The session, if there was one, is ended and the cookie cleared */
            204: {
                headers: {
                    /** @description Clears `__Host-sc_session` (`Max-Age=0`) */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content?: never;
            };
            403: components["responses"]["CsrfRefused"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    finishProviderSignIn: {
        parameters: {
            query?: {
                /** @description The authorization code, on success. */
                code?: string;
                /** @description The `state` the start route sent. */
                state?: string;
                /** @description The provider's error code, on failure. */
                error?: string;
                /** @description The provider's description of the error. Logged, never shown. */
                error_description?: string;
            };
            header?: never;
            path: {
                /** @description The provider's key in the organization (`identity_providers.key`). */
                provider: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description To the flow's `return_to`, signed in, or to `/sign-in?error=<code>` */
            303: {
                headers: {
                    /** @description `no-store` */
                    "Cache-Control"?: string;
                    /** @description The `return_to` path, or the sign-in page with the error code */
                    Location?: string;
                    /** @description With `auth.rate_limited`: seconds to wait, at least 1 */
                    "Retry-After"?: number;
                    /** @description Clears the `__Host-sc_flow` cookie; signed in, also sets `__Host-sc_session` */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    startProviderSignIn: {
        parameters: {
            query?: {
                /**
                 * @description The path on this site to land on after sign-in. Anything but a safe path on this host
                 *     becomes `/`.
                 */
                return_to?: string;
            };
            header?: never;
            path: {
                /** @description The provider's key in the organization (`identity_providers.key`). */
                provider: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description To the provider's authorization endpoint, or to `/sign-in?error=<code>` */
            303: {
                headers: {
                    /** @description `no-store` */
                    "Cache-Control"?: string;
                    /** @description The authorization URL, or the sign-in page with the error code */
                    Location?: string;
                    /** @description With `auth.rate_limited`: seconds to wait, at least 1 */
                    "Retry-After"?: number;
                    /** @description To the provider: the `__Host-sc_flow` cookie of the sign-in flow */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    healthz: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The server is running */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Health"];
                };
            };
        };
    };
    getWebManifest: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The organization's web app manifest. `Cache-Control: no-cache`. */
            200: {
                headers: {
                    /** @description `no-cache` */
                    "Cache-Control"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/manifest+json": components["schemas"]["WebManifest"];
                };
            };
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    authorizeApp: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The confirmation page naming the app, the user and the organization */
            200: {
                headers: {
                    /** @description `no-store` */
                    "Cache-Control"?: string;
                    /** @description The `__Host-sc_flow` cookie of the request */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "text/html": string;
                };
            };
            /** @description To the redirect URI with an RFC 6749 error, to the sign-in page with the request as `return_to`, or to `/sign-in?error=auth.rate_limited` */
            303: {
                headers: {
                    /** @description Where the browser goes */
                    Location?: string;
                    /** @description With `auth.rate_limited`: seconds to wait, at least 1 */
                    "Retry-After"?: number;
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description An HTML error page: `oauth.client_unknown` or `oauth.redirect_uri_mismatch` */
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "text/html": string;
                };
            };
            403: components["responses"]["AuthorizeForbidden"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    answerAppAuthorization: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description To the redirect URI with `code` and `state`, or with `error=access_denied`; or to `/sign-in?error=auth.rate_limited` */
            303: {
                headers: {
                    /** @description Where the browser goes */
                    Location?: string;
                    /** @description With `auth.rate_limited`: seconds to wait, at least 1 */
                    "Retry-After"?: number;
                    /** @description Clears `__Host-sc_flow` */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description An HTML error page: `request.malformed`, `auth.flow_invalid` or `auth.flow_expired` */
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "text/html": string;
                };
            };
            /** @description An HTML error page: `auth.unauthenticated` */
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "text/html": string;
                };
            };
            403: components["responses"]["AnswerForbidden"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    revokeAppToken: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/x-www-form-urlencoded": components["schemas"]["RevokeForm"];
            };
        };
        responses: {
            /** @description The token's family is revoked, or the token was not one of this client's */
            200: {
                headers: {
                    /** @description `no-store` */
                    "Cache-Control"?: string;
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description An RFC 6749 section 5.2 error */
            400: {
                headers: {
                    /** @description `no-store` */
                    "Cache-Control"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["OAuthError"];
                };
            };
            404: components["responses"]["OrganizationNotFound"];
            429: components["responses"]["RateLimited"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    requestAppToken: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/x-www-form-urlencoded": components["schemas"]["TokenForm"];
            };
        };
        responses: {
            /** @description The access token (15 minutes) and the next refresh token */
            200: {
                headers: {
                    /** @description `no-store` */
                    "Cache-Control"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["TokenResponse"];
                };
            };
            /** @description An RFC 6749 section 5.2 error */
            400: {
                headers: {
                    /** @description `no-store` */
                    "Cache-Control"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["OAuthError"];
                };
            };
            404: components["responses"]["OrganizationNotFound"];
            429: components["responses"]["RateLimited"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    readyz: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The database answered */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Health"];
                };
            };
            /** @description The database did not answer in time */
            503: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Health"];
                };
            };
        };
    };
    getThemeCss: {
        parameters: {
            query?: {
                /**
                 * @description The stylesheet's SHA-256 (lowercase hex), as the served page links it. When it is the
                 *     served stylesheet's, the answer is cached for a year; otherwise it is revalidated on
                 *     every load.
                 */
                v?: string;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The organization's light, dark and high-contrast themes as CSS variables: the default brand's when it has saved no branding or the instance has no organization yet. Cached for a year (`Cache-Control: public, max-age=31536000, immutable`) when `v` is the stylesheet's hash, as the page the server serves links it; revalidated on every load (`no-cache`) otherwise. */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "text/css": string;
                };
            };
            400: components["responses"]["BadRequest"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
        };
    };
}
