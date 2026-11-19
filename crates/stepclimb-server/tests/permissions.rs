// SPDX-License-Identifier: AGPL-3.0-or-later
//! The permission suite (docs/specs/m0/09-permissions-and-roles.md B19 to B21, T15, T16).
//!
//! Every operation of the OpenAPI document is either in [`PUBLIC`] or has an [`Entry`] in
//! [`TABLE`]; the walker test fails, naming each, on an operation in neither. From each entry the
//! runner generates the cases of B20 against the whole router:
//!
//! - **401**: no credential;
//! - **403**: a custom role with every catalog permission but `P`, at the target's scope;
//! - **404**: a custom role with every catalog permission at a sibling account of the target
//!   (not applicable to `OrgTarget`, `AnyScope` lists and organization-only permissions);
//! - **2xx**: a custom role with exactly `P` (and the entry's extra permissions) at the target's
//!   scope;
//! - **cross-organization**: an Organization admin of org A calls with the target built in org
//!   B: 404 for a route with a target, 2xx acting on org A otherwise; org B's rows are unchanged
//!   and none of their ids is in the response.
//!
//! An `AuthOnly` entry (a route that needs a principal and no permission) gets the 401, 2xx and
//! cross-organization cases. An `Own` entry (an `AuthOnly` route on a row of the caller's own,
//! such as `DELETE /api/v1/me/identities/{identity_id}`) gets the 401 and 2xx cases, an
//! **other-user** case (the same organization's other user's row: 404) and a
//! cross-organization case that names a row of org B's user (404).
//!
//! A `Mine` entry (an `AuthOnly` route whose request needs rows of the caller's own but names
//! none of them, such as adding a passkey, which needs the caller's local account and the
//! registration their browser started) gets the 401, 2xx, every-feature-off and
//! cross-organization cases of an `AuthOnly` entry, with the setup building those rows for the
//! user who calls: in the cross-organization case, org A's admin, acting on org A.
//!
//! An entry made with [`gated`] names the feature its route also takes as `Require<F>`
//! (docs/specs/m0/10-entitlements-framework.md B9, issue 4) and gets two more cases, run with
//! the organization on a plan that has every catalog feature but that one:
//!
//! - **feature off**: a user holding every permission at the target's scope gets 403
//!   `entitlement.feature_unavailable`, and the organization's rows are unchanged;
//! - **feature off, unknown ids** (a route with path parameters): the same, with every path
//!   parameter naming an id that does not exist, since the feature is checked before the
//!   target is resolved.
//!
//! An entry that names no feature gets **every feature off** instead: its 2xx request, run with
//! the organization on a plan with no catalog feature, must not answer
//! `entitlement.feature_unavailable`, so a route cannot check a feature its entry does not
//! declare.
//!
//! A walker test checks that the 403 of an operation documents `entitlement.feature_unavailable`
//! exactly when its entry names a feature.
//!
//! The lock suite (M1 spec 04 T13), which walks the same document for the routes that name an
//! item or its content, is `tests/lock_suite/mod.rs`.
//!
//! Each case builds its own organizations in the test's database, so cases cannot disturb each
//! other. The request names its organization with the `test-support` header of `request_org`
//! (the production resolver refuses every request while two organizations exist); the e2e test
//! `web/e2e/smoke.spec.ts` shows the release image ignores that header. Needs `DATABASE_URL`
//! (`just up`).

use std::{
    future::Future,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    pin::Pin,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Extension, Router,
    body::Body,
    extract::{ConnectInfo, State},
    http::{HeaderMap, Method, Request, StatusCode, header},
    routing::{get, patch, post},
};
use http_body_util::BodyExt;
use object_store::ObjectStoreExt as _;
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope, Source, accounts,
    admission_webhooks::{self, Change, Fresh},
    assignments,
    audit::Actor,
    clock::ManualClock,
    course_files, courses,
    feature::{
        self, AdmissionWebhook, ApiKeys, CustomBranding, CustomRoles, Feature, Limit, OidcProviders,
    },
    files,
    grants::ScopeId,
    id::{Uuid, new_id},
    identities::{self, NewIdentity},
    items, modules, organizations,
    page_doc::PageDoc,
    pages,
    perm::{
        self, AccountManage, AccountView, AdmissionManage, AssignmentManage, BrandingManage,
        CourseContentEdit, CourseCreate, CourseManage, CourseParticipate, CourseView,
        CourseViewAll, EnrollmentManage, EnrollmentView, OrgManage, Permission, ProgressView,
        ProgressViewObserved, ProviderManage, RoleAssign, RoleManage, RoleView, ScopeType,
        ServiceAccountManage, UserManage, UserView,
    },
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
    service_accounts,
    users::{self, NewUser},
    videos,
};
use stepclimb_server::{
    AppState,
    config::StorageConfig,
    error::{ApiError, Problem},
    extract::{ApiJson, ApiPath},
    openapi,
    request_org::TEST_ORG_HEADER,
    require::{AccountParentTarget, AccountTarget, AnyScope, Require},
    router,
    session::{self, Authenticated},
    storage::Storage,
    web_origin::WebOrigin,
};
use tower::ServiceExt;
use webauthn_authenticator_rs::WebauthnAuthenticator;

mod lock_suite;

/// The client address every test request comes from (TEST-NET-1); audit entries record it.
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

// The table.

/// The operations that need no credential: the probes, the unauthenticated routes specs 04
/// to 08 declare (B21) and the web app manifest (spec m1/15 B1). The OAuth 2.1 authorization
/// endpoint (`authorizeApp`, `answerAppAuthorization`) needs no permission (spec 08
/// Permissions); it answers a missing session with sign-in or its error page rather than a 401
/// problem, and
/// `tests/oauth_authorize.rs` covers its session, bearer and cross-organization cases. The token
/// and revocation endpoints (`requestAppToken`, `revokeAppToken`) take no credential at all (the
/// clients are public); `tests/oauth_token.rs` covers their cases.
const PUBLIC: &[&str] = &[
    "healthz",
    "readyz",
    "signOut",
    "getThemeCss",
    "getWebManifest",
    "getDiscovery",
    "startEnrollment",
    "enrollPasskey",
    "startPasskeySignIn",
    "finishPasskeySignIn",
    "enrollPassword",
    "signInWithPassword",
    "signInWithTotp",
    "enrollTotpAtSignIn",
    "startProviderSignIn",
    "finishProviderSignIn",
    "authorizeApp",
    "answerAppAuthorization",
    "requestAppToken",
    "revokeAppToken",
    "getSignedFile",
];

/// One entry per operation that needs a principal (B20). The M0 specs' routes add theirs here
/// as they land.
const TABLE: &[Entry] = &[
    Entry {
        operation_id: "getMe",
        access: Access::AuthOnly(at_org),
    },
    Entry {
        operation_id: "updateMyAppearance",
        access: Access::AuthOnly(dark_compact),
    },
    Entry {
        operation_id: "getOrganization",
        access: Access::AuthOnly(at_org),
    },
    Entry {
        operation_id: "updateOrganization",
        access: permission(OrgManage::KEY, Target::Org, rename_organization),
    },
    Entry {
        operation_id: "getOrganizationSettings",
        access: permission(OrgManage::KEY, Target::Org, at_org),
    },
    Entry {
        operation_id: "updateOrganizationSettings",
        access: permission(OrgManage::KEY, Target::Org, settings_patch),
    },
    Entry {
        operation_id: "getOrganizationEntitlements",
        access: permission(OrgManage::KEY, Target::Org, at_org),
    },
    Entry {
        operation_id: "listAccounts",
        access: permission(AccountView::KEY, Target::AnyScope, at_org),
    },
    Entry {
        operation_id: "getAccount",
        access: permission(AccountView::KEY, ACCOUNT_NOT_FOUND, tower_in_path),
    },
    Entry {
        operation_id: "createAccount",
        access: permission(
            AccountManage::KEY,
            ACCOUNT_NOT_FOUND,
            new_account_under_tower,
        ),
    },
    Entry {
        operation_id: "updateAccount",
        access: permission(AccountManage::KEY, ACCOUNT_NOT_FOUND, rename_tower),
    },
    Entry {
        operation_id: "moveAccount",
        access: permission(
            AccountManage::KEY,
            ACCOUNT_NOT_FOUND,
            move_ground_within_tower,
        ),
    },
    Entry {
        operation_id: "deleteAccount",
        access: permission(AccountManage::KEY, ACCOUNT_NOT_FOUND, ground_under_tower),
    },
    Entry {
        operation_id: "listCourses",
        access: permission(CourseView::KEY, Target::AnyScope, at_org),
    },
    Entry {
        operation_id: "getCourse",
        access: permission(CourseView::KEY, COURSE_NOT_FOUND, published_course_in_tower),
    },
    Entry {
        operation_id: "createCourse",
        access: permission(CourseCreate::KEY, ACCOUNT_NOT_FOUND, new_course_in_tower),
    },
    Entry {
        operation_id: "updateCourse",
        access: permission(CourseManage::KEY, COURSE_NOT_FOUND, retitle_course_in_tower),
    },
    Entry {
        operation_id: "setCourseStatus",
        access: permission(CourseManage::KEY, COURSE_NOT_FOUND, archive_course_in_tower),
    },
    Entry {
        operation_id: "moveCourse",
        // The destination needs `course.create` (B9).
        access: Access::Permission {
            key: CourseManage::KEY,
            target: COURSE_NOT_FOUND,
            extra: &[CourseCreate::KEY],
            feature: None,
            feature_by_target: false,
            denied_also: &[],
            setup: move_course_to_ground,
        },
    },
    Entry {
        operation_id: "listModules",
        access: permission(CourseView::KEY, COURSE_NOT_FOUND, published_outline),
    },
    Entry {
        operation_id: "getModule",
        access: permission(CourseView::KEY, MODULE_NOT_FOUND, published_module),
    },
    Entry {
        operation_id: "getItem",
        access: permission(CourseView::KEY, ITEM_NOT_FOUND, published_item),
    },
    Entry {
        operation_id: "createModule",
        access: permission(
            CourseContentEdit::KEY,
            COURSE_NOT_FOUND,
            new_module_in_tower_course,
        ),
    },
    Entry {
        operation_id: "reorderModules",
        access: permission(CourseContentEdit::KEY, COURSE_NOT_FOUND, reverse_modules),
    },
    Entry {
        operation_id: "updateModule",
        access: permission(CourseContentEdit::KEY, MODULE_NOT_FOUND, retitle_module),
    },
    Entry {
        operation_id: "deleteModule",
        access: permission(CourseContentEdit::KEY, MODULE_NOT_FOUND, empty_module),
    },
    Entry {
        operation_id: "createItem",
        access: permission(
            CourseContentEdit::KEY,
            MODULE_NOT_FOUND,
            new_header_in_module,
        ),
    },
    Entry {
        operation_id: "reorderItems",
        access: permission(CourseContentEdit::KEY, MODULE_NOT_FOUND, reverse_items),
    },
    Entry {
        operation_id: "updateItem",
        access: permission(CourseContentEdit::KEY, ITEM_NOT_FOUND, retitle_item),
    },
    Entry {
        operation_id: "moveItem",
        access: permission(
            CourseContentEdit::KEY,
            ITEM_NOT_FOUND,
            move_item_to_second_module,
        ),
    },
    Entry {
        operation_id: "deleteItem",
        access: permission(CourseContentEdit::KEY, ITEM_NOT_FOUND, item_in_module),
    },
    Entry {
        operation_id: "recordItemProgress",
        // `course.participate` is grantable at course scope only, so the role is held through
        // an enrollment and there is no sibling-account case; tests/progress.rs has that one.
        access: permission(CourseParticipate::KEY, ITEM_NOT_FOUND, view_published_link),
    },
    // The staff progress reads take `Require<CourseView>` and decide in the handler (M1 spec 05
    // B20): one entry per granting permission (T14a). `progress.view_observed` is grantable at
    // course scope only, so its entries hold it through an enrollment that observes the
    // learner, have no sibling-account case (tests/progress.rs has the 404), and their 403
    // also leaves out `progress.view`, which grants the route too.
    Entry {
        operation_id: "listCourseProgress",
        access: Access::Permission {
            key: ProgressView::KEY,
            target: COURSE_NOT_FOUND,
            extra: &[CourseView::KEY],
            feature: None,
            feature_by_target: false,
            denied_also: &[],
            setup: learner_of_tower_course,
        },
    },
    Entry {
        operation_id: "listCourseProgress",
        access: Access::Permission {
            key: ProgressViewObserved::KEY,
            target: COURSE_NOT_FOUND,
            extra: &[CourseView::KEY],
            feature: None,
            feature_by_target: false,
            denied_also: &[ProgressView::KEY],
            setup: observed_learner_of_tower_course,
        },
    },
    Entry {
        operation_id: "getLearnerProgress",
        access: Access::Permission {
            key: ProgressView::KEY,
            target: COURSE_NOT_FOUND,
            extra: &[CourseView::KEY],
            feature: None,
            feature_by_target: false,
            denied_also: &[],
            setup: learner_of_tower_course,
        },
    },
    Entry {
        operation_id: "getLearnerProgress",
        access: Access::Permission {
            key: ProgressViewObserved::KEY,
            target: COURSE_NOT_FOUND,
            extra: &[CourseView::KEY],
            feature: None,
            feature_by_target: false,
            denied_also: &[ProgressView::KEY],
            setup: observed_learner_of_tower_course,
        },
    },
    // The course's completion history decides in the handler as the staff progress reads do
    // (M1 spec 14 B13, T10a): one entry per granting permission.
    Entry {
        operation_id: "listCourseCompletions",
        access: Access::Permission {
            key: ProgressView::KEY,
            target: COURSE_NOT_FOUND,
            extra: &[CourseView::KEY],
            feature: None,
            feature_by_target: false,
            denied_also: &[],
            setup: completion_of_tower_learner,
        },
    },
    Entry {
        operation_id: "listCourseCompletions",
        access: Access::Permission {
            key: ProgressViewObserved::KEY,
            target: COURSE_NOT_FOUND,
            extra: &[CourseView::KEY],
            feature: None,
            feature_by_target: false,
            denied_also: &[ProgressView::KEY],
            setup: observed_completion_of_tower_learner,
        },
    },
    Entry {
        operation_id: "listMyCompletions",
        access: Access::AuthOnly(at_org),
    },
    Entry {
        operation_id: "copyCourse",
        // The source needs `course.view_all` and the destination `course.create` (B12).
        access: Access::Permission {
            key: CourseViewAll::KEY,
            target: COURSE_NOT_FOUND,
            extra: &[CourseCreate::KEY],
            feature: None,
            feature_by_target: false,
            denied_also: &[],
            setup: copy_course_to_ground,
        },
    },
    Entry {
        operation_id: "listEnrollments",
        access: permission(EnrollmentView::KEY, COURSE_NOT_FOUND, course_in_tower),
    },
    Entry {
        operation_id: "createEnrollment",
        // The user enrolled must be covered by the caller's `user.view` (M1 spec 02 B17, owner
        // decision of 2026-11-17).
        access: Access::Permission {
            key: EnrollmentManage::KEY,
            target: COURSE_NOT_FOUND,
            extra: &[UserView::KEY],
            feature: None,
            feature_by_target: false,
            denied_also: &[],
            setup: participant_for_tower_course,
        },
    },
    Entry {
        operation_id: "updateEnrollment",
        access: permission(
            EnrollmentManage::KEY,
            ENROLLMENT_NOT_FOUND,
            conclude_enrollment,
        ),
    },
    Entry {
        operation_id: "deleteEnrollment",
        access: permission(
            EnrollmentManage::KEY,
            ENROLLMENT_NOT_FOUND,
            enrollment_in_path,
        ),
    },
    Entry {
        operation_id: "listAssignments",
        access: permission(AssignmentManage::KEY, COURSE_NOT_FOUND, course_in_tower),
    },
    Entry {
        // The assignee is a learner of the course, so assigning them enrolls no one and needs
        // no `enrollment.manage` (M1 spec 13 B3); tests/assignments.rs has that case.
        operation_id: "createAssignment",
        access: permission(
            AssignmentManage::KEY,
            COURSE_NOT_FOUND,
            assign_tower_learner,
        ),
    },
    Entry {
        operation_id: "getAssignment",
        access: permission(
            AssignmentManage::KEY,
            ASSIGNMENT_NOT_FOUND,
            assignment_in_path,
        ),
    },
    Entry {
        operation_id: "updateAssignment",
        access: permission(
            AssignmentManage::KEY,
            ASSIGNMENT_NOT_FOUND,
            move_assignment_due,
        ),
    },
    Entry {
        operation_id: "endAssignment",
        access: permission(
            AssignmentManage::KEY,
            ASSIGNMENT_NOT_FOUND,
            assignment_in_path,
        ),
    },
    Entry {
        operation_id: "listCourseFiles",
        access: permission(CourseViewAll::KEY, COURSE_NOT_FOUND, files_of_tower_course),
    },
    Entry {
        operation_id: "uploadCourseFile",
        access: permission(
            CourseContentEdit::KEY,
            COURSE_NOT_FOUND,
            upload_into_tower_course,
        ),
    },
    Entry {
        operation_id: "getCourseFile",
        access: permission(CourseViewAll::KEY, FILE_NOT_FOUND, course_file_in_tower),
    },
    Entry {
        operation_id: "createCourseFileUrl",
        access: permission(
            CourseViewAll::KEY,
            FILE_NOT_FOUND,
            url_of_course_file_in_tower,
        ),
    },
    Entry {
        operation_id: "createItemFileUrl",
        access: permission(CourseView::KEY, ITEM_NOT_FOUND, url_of_published_file_item),
    },
    Entry {
        operation_id: "updateCourseFile",
        access: permission(CourseContentEdit::KEY, FILE_NOT_FOUND, rename_course_file),
    },
    Entry {
        operation_id: "deleteCourseFile",
        access: permission(CourseContentEdit::KEY, FILE_NOT_FOUND, course_file_in_tower),
    },
    Entry {
        operation_id: "renameCourseFolder",
        access: permission(
            CourseContentEdit::KEY,
            COURSE_NOT_FOUND,
            rename_tower_folder,
        ),
    },
    Entry {
        operation_id: "listPages",
        access: permission(CourseViewAll::KEY, COURSE_NOT_FOUND, pages_of_tower_course),
    },
    Entry {
        operation_id: "createPage",
        access: permission(CourseContentEdit::KEY, COURSE_NOT_FOUND, new_page_in_tower),
    },
    Entry {
        operation_id: "updatePage",
        access: permission(CourseContentEdit::KEY, PAGE_NOT_FOUND, retitle_page),
    },
    Entry {
        operation_id: "savePageBody",
        access: permission(CourseContentEdit::KEY, PAGE_NOT_FOUND, save_page_body),
    },
    Entry {
        operation_id: "deletePage",
        access: permission(CourseContentEdit::KEY, PAGE_NOT_FOUND, page_in_tower),
    },
    Entry {
        operation_id: "listPageVersions",
        access: permission(CourseViewAll::KEY, PAGE_NOT_FOUND, page_in_tower),
    },
    Entry {
        operation_id: "getPageVersion",
        access: permission(CourseViewAll::KEY, PAGE_NOT_FOUND, page_version_in_tower),
    },
    Entry {
        operation_id: "restorePageVersion",
        access: permission(CourseContentEdit::KEY, PAGE_NOT_FOUND, restore_page_version),
    },
    Entry {
        operation_id: "listCourseVideos",
        access: permission(CourseViewAll::KEY, COURSE_NOT_FOUND, videos_of_tower_course),
    },
    Entry {
        operation_id: "createVideoUpload",
        access: permission(CourseContentEdit::KEY, COURSE_NOT_FOUND, tower_course),
    },
    Entry {
        operation_id: "getVideoUploadOffset",
        access: permission(
            CourseContentEdit::KEY,
            VIDEO_UPLOAD_NOT_FOUND,
            video_upload_in_tower,
        ),
    },
    Entry {
        operation_id: "appendVideoUpload",
        access: permission(
            CourseContentEdit::KEY,
            VIDEO_UPLOAD_NOT_FOUND,
            chunk_of_video_upload_in_tower,
        ),
    },
    Entry {
        operation_id: "deleteVideoUpload",
        access: permission(
            CourseContentEdit::KEY,
            VIDEO_UPLOAD_NOT_FOUND,
            video_upload_in_tower,
        ),
    },
    Entry {
        operation_id: "getVideo",
        access: permission(CourseViewAll::KEY, VIDEO_NOT_FOUND, video_in_tower),
    },
    Entry {
        operation_id: "updateVideo",
        access: permission(
            CourseContentEdit::KEY,
            VIDEO_NOT_FOUND,
            rename_video_in_tower,
        ),
    },
    Entry {
        operation_id: "retryVideo",
        access: permission(
            CourseContentEdit::KEY,
            VIDEO_NOT_FOUND,
            failed_video_in_tower,
        ),
    },
    Entry {
        operation_id: "listPermissions",
        access: permission(RoleView::KEY, Target::AnyScope, at_org),
    },
    Entry {
        operation_id: "listRoles",
        access: permission(RoleView::KEY, Target::AnyScope, at_org),
    },
    Entry {
        operation_id: "getRole",
        access: permission(RoleView::KEY, ROLE_NOT_FOUND, custom_role_in_path),
    },
    Entry {
        operation_id: "createRole",
        access: gated(
            CustomRoles::KEY,
            permission(RoleManage::KEY, Target::Org, new_role_in_body),
        ),
    },
    Entry {
        operation_id: "updateRole",
        access: gated(
            CustomRoles::KEY,
            permission(RoleManage::KEY, ROLE_NOT_FOUND, rename_custom_role),
        ),
    },
    Entry {
        operation_id: "deleteRole",
        access: gated(
            CustomRoles::KEY,
            permission(RoleManage::KEY, ROLE_NOT_FOUND, custom_role_in_path),
        ),
    },
    Entry {
        operation_id: "listRoleAssignments",
        access: permission(RoleView::KEY, Target::AnyScope, at_org),
    },
    Entry {
        operation_id: "createRoleAssignment",
        // The user is checked first, so another organization's request ends at its user.
        access: permission(
            RoleAssign::KEY,
            Target::Scoped {
                not_found: "account.not_found",
                cross_org: "user.not_found",
            },
            assignment_at_tower_in_body,
        ),
    },
    Entry {
        operation_id: "deleteRoleAssignment",
        access: permission(
            RoleAssign::KEY,
            Target::Scoped {
                not_found: "role_assignment.not_found",
                cross_org: "role_assignment.not_found",
            },
            assignment_at_tower_in_path,
        ),
    },
    Entry {
        operation_id: "listUsers",
        // Account-scope `user.view` lists the users in its subtrees (M1 spec 02 B21).
        access: permission(UserView::KEY, Target::AnyScope, at_org),
    },
    Entry {
        operation_id: "getUser",
        access: permission(UserView::KEY, USER_NOT_FOUND, user_in_path),
    },
    Entry {
        operation_id: "createUser",
        access: permission(UserManage::KEY, Target::Org, new_user_in_body),
    },
    Entry {
        operation_id: "updateUser",
        access: permission(UserManage::KEY, USER_NOT_FOUND, patch_user_in_path),
    },
    Entry {
        operation_id: "createEnrollmentLink",
        access: permission(UserManage::KEY, USER_NOT_FOUND, user_with_email_in_path),
    },
    Entry {
        operation_id: "removeUserTotp",
        access: permission(UserManage::KEY, USER_NOT_FOUND, user_with_totp_in_path),
    },
    Entry {
        operation_id: "listUserIdentities",
        access: permission(UserView::KEY, USER_NOT_FOUND, user_in_path),
    },
    Entry {
        operation_id: "createUserIdentity",
        access: permission(UserManage::KEY, USER_NOT_FOUND, new_identity_for_user),
    },
    Entry {
        operation_id: "deleteUserIdentity",
        access: permission(UserManage::KEY, USER_NOT_FOUND, second_identity_of_user),
    },
    Entry {
        operation_id: "listMyIdentities",
        access: Access::AuthOnly(at_org),
    },
    Entry {
        operation_id: "deleteMyIdentity",
        access: Access::Own {
            not_found: "identity.not_found",
            setup: second_identity_of_owner,
        },
    },
    Entry {
        operation_id: "listMyPasskeys",
        access: Access::Mine(passkey_of_caller),
    },
    Entry {
        operation_id: "startMyPasskey",
        access: Access::Mine(local_account_of_caller),
    },
    Entry {
        operation_id: "createMyPasskey",
        access: Access::Mine(registration_of_caller),
    },
    Entry {
        operation_id: "updateMyPasskey",
        access: Access::Own {
            not_found: "passkey.not_found",
            setup: rename_passkey_of_owner,
        },
    },
    Entry {
        operation_id: "deleteMyPasskey",
        access: Access::Own {
            not_found: "passkey.not_found",
            setup: second_passkey_of_owner,
        },
    },
    Entry {
        operation_id: "changeMyPassword",
        access: Access::Mine(password_of_caller),
    },
    Entry {
        operation_id: "startMyTotp",
        access: Access::Mine(password_for_totp),
    },
    Entry {
        operation_id: "confirmMyTotp",
        access: Access::Mine(totp_waiting_for_caller),
    },
    Entry {
        operation_id: "disableMyTotp",
        access: Access::Mine(totp_of_caller),
    },
    Entry {
        operation_id: "getBranding",
        access: permission(BrandingManage::KEY, Target::Org, at_org),
    },
    Entry {
        operation_id: "updateBranding",
        access: gated(
            CustomBranding::KEY,
            permission(BrandingManage::KEY, Target::Org, new_branding),
        ),
    },
    Entry {
        operation_id: "getLocalSignIn",
        access: permission(ProviderManage::KEY, Target::Org, at_org),
    },
    Entry {
        operation_id: "updateLocalSignIn",
        access: permission(ProviderManage::KEY, Target::Org, unchanged_local_sign_in),
    },
    Entry {
        operation_id: "listServiceAccounts",
        access: gated(
            ApiKeys::KEY,
            permission(ServiceAccountManage::KEY, Target::Org, at_org),
        ),
    },
    Entry {
        operation_id: "createServiceAccount",
        access: gated(
            ApiKeys::KEY,
            permission(ServiceAccountManage::KEY, Target::Org, new_service_account),
        ),
    },
    Entry {
        operation_id: "deleteServiceAccount",
        access: gated(
            ApiKeys::KEY,
            permission(
                ServiceAccountManage::KEY,
                SERVICE_ACCOUNT_NOT_FOUND,
                service_account_in_path,
            ),
        ),
    },
    Entry {
        operation_id: "getAdmissionSettings",
        access: permission(AdmissionManage::KEY, Target::Org, at_org),
    },
    Entry {
        operation_id: "updateAdmissionSettings",
        access: permission(AdmissionManage::KEY, Target::Org, open_signup),
    },
    Entry {
        operation_id: "getAdmissionWebhook",
        access: permission(AdmissionManage::KEY, WEBHOOK_NOT_FOUND, webhook_of_org),
    },
    Entry {
        operation_id: "updateAdmissionWebhook",
        access: gated(
            AdmissionWebhook::KEY,
            with_every_permission(permission(
                AdmissionManage::KEY,
                Target::Org,
                move_webhook_of_org,
            )),
        ),
    },
    Entry {
        operation_id: "rotateAdmissionWebhookSecret",
        access: gated(
            AdmissionWebhook::KEY,
            with_every_permission(permission(
                AdmissionManage::KEY,
                WEBHOOK_NOT_FOUND,
                webhook_of_org,
            )),
        ),
    },
    Entry {
        operation_id: "deleteAdmissionWebhook",
        access: with_every_permission(permission(
            AdmissionManage::KEY,
            WEBHOOK_NOT_FOUND,
            webhook_of_org,
        )),
    },
    Entry {
        operation_id: "listProviders",
        access: permission(ProviderManage::KEY, Target::Org, at_org),
    },
    Entry {
        operation_id: "getProvider",
        access: permission(ProviderManage::KEY, PROVIDER_NOT_FOUND, provider_in_path),
    },
    Entry {
        operation_id: "createProvider",
        access: gated(
            OidcProviders::KEY,
            permission(ProviderManage::KEY, Target::Org, new_oidc_provider),
        ),
    },
    Entry {
        operation_id: "updateProvider",
        access: gated_by_target(
            OidcProviders::KEY,
            permission(ProviderManage::KEY, PROVIDER_NOT_FOUND, relabel_provider),
        ),
    },
    Entry {
        operation_id: "deleteProvider",
        access: permission(ProviderManage::KEY, PROVIDER_NOT_FOUND, provider_in_path),
    },
];

/// The organization has no admission webhook. The routes act on the caller's organization's
/// webhook, so org A's admin calling with org B's webhook in place finds none.
const WEBHOOK_NOT_FOUND: Target = Target::Scoped {
    not_found: "admission.webhook_not_found",
    cross_org: "admission.webhook_not_found",
};

/// Every catalog permission key: the 2xx case of a write that the escalation rule guards
/// (spec 07 I5) holds them all. `every_permission_lists_the_catalog` keeps it complete.
const EVERY_PERMISSION: &[&str] = &[
    "organization.manage",
    "account.view",
    "account.manage",
    "user.view",
    "user.manage",
    "role.view",
    "role.manage",
    "role.assign",
    "provider.manage",
    "admission.manage",
    "service_account.manage",
    "branding.manage",
    "course.view",
    "course.view_all",
    "course.create",
    "course.manage",
    "course.content_edit",
    "enrollment.view",
    "enrollment.manage",
    "course.participate",
    "progress.view",
    "progress.view_observed",
    "assignment.manage",
];

#[test]
fn every_permission_lists_the_catalog() {
    let mut listed = EVERY_PERMISSION.to_vec();
    let mut catalog = all_keys();
    listed.sort_unstable();
    catalog.sort_unstable();
    assert_eq!(listed, catalog);
}

/// `access`, a [`permission`] entry, whose 2xx case also holds every other catalog permission
/// ([`with_extra`] leaves out the entry's own key).
const fn with_every_permission(access: Access) -> Access {
    match access {
        Access::Permission {
            key,
            target,
            feature,
            feature_by_target,
            setup,
            ..
        } => Access::Permission {
            key,
            target,
            extra: EVERY_PERMISSION,
            feature,
            feature_by_target,
            denied_also: &[],
            setup,
        },
        Access::AuthOnly(_) | Access::Own { .. } | Access::Mine(_) => {
            panic!("every permission needs a permission entry")
        }
    }
}

/// An admission webhook of `org`, its URL naming the organization so the cross-organization
/// case sees it if it leaks.
fn webhook_of_org(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        org.admission_webhook().await;
        Call {
            params: Vec::new(),
            body: None,
            at: At::Org,
        }
    })
}

const PROVIDER_NOT_FOUND: Target = Target::Scoped {
    not_found: "provider.not_found",
    cross_org: "provider.not_found",
};

/// A new `oidc` provider of `org` (`Org::provider`), named in the path.
fn provider_in_path(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let (id, _) = org.provider().await;
        Call {
            params: vec![("provider_id", id)],
            body: None,
            at: At::Org,
        }
    })
}

/// [`webhook_of_org`] and a body that moves it: an update, and in an organization without one
/// (the cross-organization case's org A), a creation.
fn move_webhook_of_org(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        org.admission_webhook().await;
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({ "url": "https://suite.example.test/hook" })),
            at: At::Org,
        }
    })
}

/// A new label for a new `oidc` provider of `org`.
fn relabel_provider(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let (id, _) = org.provider().await;
        Call {
            params: vec![("provider_id", id)],
            body: Some(serde_json::json!({ "label": "Renamed by the suite" })),
            at: At::Org,
        }
    })
}

/// A new, disabled `oidc` provider: `provider.manage` alone may create it (sign-up is off in
/// a new organization, and it is not trusted for email matching).
fn new_oidc_provider(_: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({
                "key": "suite-sso",
                "kind": "oidc",
                "label": "Suite SSO",
                "enabled": false,
                "config": {
                    "issuer": "https://idp.example.test/suite",
                    "client_id": "suite",
                    "client_secret": "suite-secret-value",
                },
            })),
            at: At::Org,
        }
    })
}

/// A passkey of the caller, so the list has a row to answer.
fn passkey_of_caller(org: &Org, caller: Uuid) -> Pin<Box<dyn Future<Output = Mine> + Send + '_>> {
    Box::pin(async move {
        org.passkey(caller).await;
        Mine::call(Call {
            params: Vec::new(),
            body: None,
            at: At::Org,
        })
    })
}

/// The caller's password, and the body that changes it.
fn password_of_caller(org: &Org, caller: Uuid) -> Pin<Box<dyn Future<Output = Mine> + Send + '_>> {
    Box::pin(async move {
        let hash = stepclimb_auth::password::hash("correct horse battery staple").expect("a hash");
        stepclimb_core::local_logins::set_password(
            &org.pool,
            scope(org.id),
            caller,
            hash.as_str(),
            now(),
            Actor::Instance,
        )
        .await
        .expect("set the password");
        Mine::call(Call {
            params: Vec::new(),
            body: Some(serde_json::json!({
                "current_password": "correct horse battery staple",
                "new_password": "tremendous violet umbrella",
            })),
            at: At::Org,
        })
    })
}

/// The caller's password, which TOTP follows.
fn password_for_totp(org: &Org, caller: Uuid) -> Pin<Box<dyn Future<Output = Mine> + Send + '_>> {
    Box::pin(async move {
        give_password(org, caller).await;
        Mine::call(Call {
            params: Vec::new(),
            body: None,
            at: At::Org,
        })
    })
}

/// The caller's password and an unconfirmed TOTP secret, and the body with its current code.
fn totp_waiting_for_caller(
    org: &Org,
    caller: Uuid,
) -> Pin<Box<dyn Future<Output = Mine> + Send + '_>> {
    Box::pin(async move {
        let secret = give_totp(org, caller, false).await;
        Mine::call(Call {
            params: Vec::new(),
            body: Some(serde_json::json!({ "code": secret.code_at_time(now()) })),
            at: At::Org,
        })
    })
}

/// The caller's password and confirmed TOTP, and the body with its current code.
fn totp_of_caller(org: &Org, caller: Uuid) -> Pin<Box<dyn Future<Output = Mine> + Send + '_>> {
    Box::pin(async move {
        let secret = give_totp(org, caller, true).await;
        Mine::call(Call {
            params: Vec::new(),
            body: Some(serde_json::json!({ "code": secret.code_at_time(now()) })),
            at: At::Org,
        })
    })
}

/// Store the suite's password for `user` of `org`.
async fn give_password(org: &Org, user: Uuid) {
    let hash = stepclimb_auth::password::hash("correct horse battery staple").expect("a hash");
    stepclimb_core::local_logins::set_password(
        &org.pool,
        scope(org.id),
        user,
        hash.as_str(),
        now(),
        Actor::Instance,
    )
    .await
    .expect("set the password");
}

/// [`give_password`], then a new TOTP secret for `user`, sealed as the server seals it, and
/// confirmed when `confirmed`. Returns the secret, to play the app with.
async fn give_totp(org: &Org, user: Uuid, confirmed: bool) -> stepclimb_auth::totp::Secret {
    give_password(org, user).await;
    let secret = stepclimb_auth::totp::Secret::generate().expect("a secret");
    let keys = state_for(&org.pool).keys;
    let sealed =
        stepclimb_server::totp::seal_secret(&keys, scope(org.id), user, &secret).expect("seal");
    let mut conn = org.pool.acquire().await.expect("a connection");
    let s = scope(org.id);
    stepclimb_core::local_totp::start(&mut conn, s, user, &sealed, now())
        .await
        .expect("start TOTP");
    if confirmed {
        stepclimb_core::local_totp::confirm(&mut conn, s, user, &[], now(), Actor::Instance)
            .await
            .expect("confirm TOTP");
    }
    secret
}

/// The caller's local account, the identity an enrollment link creates: what lets them add a
/// passkey (spec 04 B13, the owner's provider rule).
fn local_account_of_caller(
    org: &Org,
    caller: Uuid,
) -> Pin<Box<dyn Future<Output = Mine> + Send + '_>> {
    Box::pin(async move {
        org.local_identity(caller).await;
        Mine::call(Call {
            params: Vec::new(),
            body: None,
            at: At::Org,
        })
    })
}

/// [`local_account_of_caller`], a registration started for the caller and a software passkey's
/// answer to it: the body and flow cookie `createMyPasskey` adds the passkey with.
fn registration_of_caller(
    org: &Org,
    caller: Uuid,
) -> Pin<Box<dyn Future<Output = Mine> + Send + '_>> {
    Box::pin(async move {
        org.local_identity(caller).await;
        let state = state_for(&org.pool);
        let mut conn = org.pool.acquire().await.expect("a connection");
        let begun =
            stepclimb_server::my_passkeys::begin(&state, &mut conn, scope(org.id), caller, now())
                .await
                .expect("start a registration");
        let options: webauthn_rs::prelude::CreationChallengeResponse =
            serde_json::from_value(serde_json::to_value(&begun.options).expect("JSON"))
                .expect("creation options");
        let origin = webauthn_rs::prelude::Url::parse("https://lms.example.org").expect("a URL");
        let credential = webauthn_authenticator_rs::softpasskey::SoftPasskey::new(true)
            .do_registration(origin, options)
            .expect("the soft passkey registers");
        let flow = begun.set_cookie.to_str().expect("ASCII");
        let flow = flow.split(';').next().expect("a pair").to_owned();
        Mine {
            call: Call {
                params: Vec::new(),
                body: Some(serde_json::json!({ "credential": credential })),
                at: At::Org,
            },
            flow: Some(flow),
        }
    })
}

/// A passkey of `owner` named in the path, and a new name for it.
fn rename_passkey_of_owner(
    org: &Org,
    owner: Uuid,
) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async move {
        Call {
            params: vec![("passkey_id", org.passkey(owner).await)],
            body: Some(serde_json::json!({ "name": "Renamed passkey" })),
            at: At::Org,
        }
    })
}

/// Two passkeys of `owner`, the second named in the path, so deleting it leaves a way to sign
/// in.
fn second_passkey_of_owner(
    org: &Org,
    owner: Uuid,
) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async move {
        org.passkey(owner).await;
        Call {
            params: vec![("passkey_id", org.passkey(owner).await)],
            body: None,
            at: At::Org,
        }
    })
}

/// A service account that is not the organization's: 404 `service_account.not_found`. Service
/// accounts are organization-wide, so the route has no wrong-account 404 case.
const SERVICE_ACCOUNT_NOT_FOUND: Target = Target::Scoped {
    not_found: "service_account.not_found",
    cross_org: "service_account.not_found",
};

/// A service account with no scopes, which any `service_account.manage` holder may create.
fn new_service_account(_: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({ "name": "Suite key", "scopes": [] })),
            at: At::Org,
        }
    })
}

/// A new user's service account of `org`, named in the path.
fn service_account_in_path(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let (_, owner) = org.user().await;
        Call {
            params: vec![(
                "service_account_id",
                org.service_account(owner, "Suite key").await,
            )],
            body: None,
            at: At::Org,
        }
    })
}

/// Sign-up switched on: a change, so the cross-organization case sees org A's settings and
/// audit entries move while org B's stay as they are (spec 07 T20).
fn open_signup(_: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({ "signup_allowed": true })),
            at: At::Org,
        }
    })
}

/// An appearance choice: the caller's own row, so it needs no target.
fn dark_compact(_: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({ "theme": "dark", "density": "compact" })),
            at: At::Org,
        }
    })
}

/// The local sign-in switches as a new organization has them. A `PUT` of them changes nothing,
/// so `provider.manage` alone may send it: the escalation rule applies only to a change (spec
/// 04, tests/local_sign_in.rs).
fn unchanged_local_sign_in(_: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({
                "enabled": true,
                "passkey": true,
                "password": true,
                "totp": true,
                "magic_link": false,
                "totp_required": false,
            })),
            at: At::Org,
        }
    })
}

/// A user that is not the organization's: 404 `user.not_found`. The user routes take the
/// organization as their target (spec 03 API), so they have no wrong-account 404 case.
const USER_NOT_FOUND: Target = Target::Scoped {
    not_found: "user.not_found",
    cross_org: "user.not_found",
};

/// A new user of `org`, named in the path; the caller acts at organization scope.
fn user_in_path(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let (_, user) = org.user().await;
        Call {
            params: vec![("user_id", user)],
            body: None,
            at: At::Org,
        }
    })
}

/// A new user of `org` with an email, which an enrollment link needs (spec 04 B4), named in the
/// path; the caller acts at organization scope.
fn user_with_email_in_path(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let email = format!("enroll-{}@example.test", new_id().simple());
        let (_, user) = org.named_user("Pat Example".into(), Some(email)).await;
        Call {
            params: vec![("user_id", user)],
            body: None,
            at: At::Org,
        }
    })
}

/// A new user of `org` with a password and confirmed TOTP, named in the path.
fn user_with_totp_in_path(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let (_, user) = org.user().await;
        give_totp(org, user, true).await;
        Call {
            params: vec![("user_id", user)],
            body: None,
            at: At::Org,
        }
    })
}

/// [`user_in_path`] with a new display name and a new manager, another user of `org`, in the
/// body.
fn patch_user_in_path(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let (_, manager) = org.named_user("Robin Manager".into(), None).await;
        Call {
            body: Some(
                serde_json::json!({ "display_name": "Renamed user", "manager_id": manager }),
            ),
            ..user_in_path(org).await
        }
    })
}

/// [`user_in_path`] with a new provider's subject to link in the body.
fn new_identity_for_user(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let (_, key) = org.provider().await;
        Call {
            body: Some(serde_json::json!({ "provider_key": key, "subject": "1000001" })),
            ..user_in_path(org).await
        }
    })
}

/// A new user named in the body.
fn new_user_in_body(_: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({ "display_name": "Casey Example" })),
            at: At::Org,
        }
    })
}

/// A new user with two identities, the second named in the path, so removing it leaves one.
fn second_identity_of_user(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let (_, user) = org.user().await;
        let identity = org.two_identities(user).await;
        Call {
            params: vec![("user_id", user), ("identity_id", identity)],
            body: None,
            at: At::Org,
        }
    })
}

/// Two identities of `owner`, the second named in the path.
fn second_identity_of_owner(
    org: &Org,
    owner: Uuid,
) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async move {
        Call {
            params: vec![("identity_id", org.two_identities(owner).await)],
            body: None,
            at: At::Org,
        }
    })
}

/// Branding inputs every brand check passes, the default appearance among them.
fn new_branding(_: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({
                "primary": "#500E0E",
                "ui_font": "Roboto",
                "default_appearance": "dark",
            })),
            at: At::Org,
        }
    })
}

/// A new name for the caller's organization.
fn rename_organization(_: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({ "name": "Renamed organization" })),
            at: At::Org,
        }
    })
}

/// A settings patch that names `email_matching` with its current value, so the allowed case
/// passes the setting's validation and writes nothing.
fn settings_patch(_: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({ "email_matching": false })),
            at: At::Org,
        }
    })
}

/// A new user and an unassigned custom role with no permissions, to assign at Tower: any
/// `role.assign` holder at Tower may grant it.
fn assignment_at_tower_in_body(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let (_, user) = org.user().await;
        let role = org.custom_role().await;
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({
                "user_id": user,
                "role_id": role,
                "scope": { "type": "account", "id": org.tower },
            })),
            at: At::Account(org.tower),
        }
    })
}

/// A new user's assignment of a custom role with no permissions at Tower, named in the path.
fn assignment_at_tower_in_path(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let (_, user) = org.user().await;
        let role = org.custom_role().await;
        let assignment = org.assign(user, role, At::Account(org.tower)).await;
        Call {
            params: vec![("assignment_id", assignment)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// A new account under Tower, named in the body; the caller acts at Tower.
fn new_account_under_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({ "parent_id": org.tower, "name": "Approach" })),
            at: At::Account(org.tower),
        }
    })
}

/// [`tower_in_path`] with a new name in the body.
fn rename_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            body: Some(serde_json::json!({ "name": "Renamed account" })),
            ..tower_in_path(org).await
        }
    })
}

/// Ground, checked at its parent Tower, moved under a new account beside it: the destination
/// is inside Tower's subtree, so `account.manage` at Tower covers both ends of the move.
fn move_ground_within_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let destination = org.account(org.tower, "Destination").await;
        Call {
            body: Some(serde_json::json!({ "parent_id": destination })),
            ..ground_under_tower(org).await
        }
    })
}

/// A course that is not the organization's, or that the caller cannot see: 404
/// `course.not_found` (M1 spec 02 B24).
const COURSE_NOT_FOUND: Target = Target::Scoped {
    not_found: "course.not_found",
    cross_org: "course.not_found",
};

/// A published course in Tower, named in the path; the caller acts at Tower. Published, so
/// `course.view` alone reads it (B10).
fn published_course_in_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: vec![("course_id", org.course(org.tower, true).await)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// A new course in Tower, named in the body; the caller acts at Tower.
fn new_course_in_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({
                "account_id": org.tower, "title": "Suite course", "code": "SUITE-NEW",
            })),
            at: At::Account(org.tower),
        }
    })
}

/// A published course in Tower with a new title and a completion validity (M1 spec 14 B3) in
/// the body: a draft would answer 404 to a caller without `course.view_all` (M1 spec 02 Q13),
/// and the 2xx case holds `course.manage` alone.
fn retitle_course_in_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: vec![("course_id", org.course(org.tower, true).await)],
            body: Some(serde_json::json!({
                "title": "Retitled course",
                "completion_valid_months": 12,
            })),
            at: At::Account(org.tower),
        }
    })
}

/// A published course in Tower, archived.
fn archive_course_in_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: vec![("course_id", org.course(org.tower, true).await)],
            body: Some(serde_json::json!({ "status": "archived" })),
            at: At::Account(org.tower),
        }
    })
}

/// A published course in Tower moved to Ground, inside Tower's subtree, so a grant at Tower
/// covers both ends of the move.
fn move_course_to_ground(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: vec![("course_id", org.course(org.tower, true).await)],
            body: Some(serde_json::json!({ "account_id": org.ground })),
            at: At::Account(org.tower),
        }
    })
}

/// A published course in Tower, named in the path; the caller acts at Tower. Published, so
/// the route's permission alone reaches it (M1 spec 02 B24, Q13).
fn course_in_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: vec![("course_id", org.course(org.tower, true).await)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// A user active in a Tower course, so `user.view` at Tower covers them, enrolled in another
/// published Tower course with a custom role holding only `course.participate`, which is left
/// out of the escalation check (M1 spec 02 B24a): `enrollment.manage` and `user.view` enroll
/// them.
fn participant_for_tower_course(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let first = org.course(org.tower, true).await;
        let enrollment = org.enrollment(first).await;
        let user: Uuid = sqlx::query_scalar("SELECT user_id FROM enrollments WHERE id = $1")
            .bind(enrollment)
            .fetch_one(&org.pool)
            .await
            .expect("the enrolled user");
        Call {
            params: vec![("course_id", org.course(org.tower, true).await)],
            body: Some(serde_json::json!({
                "user_id": user, "role_id": org.participant_role().await,
            })),
            at: At::Account(org.tower),
        }
    })
}

/// An enrollment that is not the organization's, or whose course the caller cannot see: 404
/// `enrollment.not_found` (M1 spec 02 API).
const ENROLLMENT_NOT_FOUND: Target = Target::Scoped {
    not_found: "enrollment.not_found",
    cross_org: "enrollment.not_found",
};

/// An enrollment in a published course in Tower, named in the path; the caller acts at Tower.
fn enrollment_in_path(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        Call {
            params: vec![("enrollment_id", org.enrollment(course).await)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// [`enrollment_in_path`], concluded.
fn conclude_enrollment(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            body: Some(serde_json::json!({ "status": "concluded" })),
            ..enrollment_in_path(org).await
        }
    })
}

/// An assignment that is not the organization's, or whose course the caller cannot see: 404
/// `assignment.not_found` (M1 spec 13 API).
const ASSIGNMENT_NOT_FOUND: Target = Target::Scoped {
    not_found: "assignment.not_found",
    cross_org: "assignment.not_found",
};

/// A published course in Tower and a learner of it (a user enrolled with a custom role holding
/// only `course.participate`) as the body's assignee, with a due date 14 days on; the caller
/// acts at Tower.
fn assign_tower_learner(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        let learner = org.learner(course).await;
        Call {
            params: vec![("course_id", course)],
            body: Some(serde_json::json!({
                "target": { "type": "course", "id": course },
                "assignee": { "type": "user", "id": learner },
                "due": { "relative_days": 14 },
            })),
            at: At::Account(org.tower),
        }
    })
}

/// An assignment of a published Tower course to a learner of it, named in the path; the caller
/// acts at Tower.
fn assignment_in_path(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        let learner = org.learner(course).await;
        let new = assignments::NewAssignment {
            target_type: assignments::TargetType::Course,
            target_id: course,
            user_id: learner,
            due: None,
            created_by: learner,
        };
        let enroll = assignments::Enroll::Refuse;
        let id = assignments::create(&org.pool, scope(org.id), course, &new, enroll, now())
            .await
            .expect("create assignment")
            .id;
        Call {
            params: vec![("assignment_id", id)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// [`assignment_in_path`] given a due date 7 days on.
fn move_assignment_due(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            body: Some(serde_json::json!({ "due": { "relative_days": 7 } })),
            ..assignment_in_path(org).await
        }
    })
}

/// A module that is not the organization's, or whose course the caller cannot see: 404
/// `module.not_found` (M1 spec 03 API).
const MODULE_NOT_FOUND: Target = Target::Scoped {
    not_found: "module.not_found",
    cross_org: "module.not_found",
};

/// An item that is not the organization's, or whose course the caller cannot see: 404
/// `item.not_found` (M1 spec 03 API).
const ITEM_NOT_FOUND: Target = Target::Scoped {
    not_found: "item.not_found",
    cross_org: "item.not_found",
};

/// A published course in Tower with a published module holding a published link. The read
/// setups publish everything: the 2xx case holds `course.view` alone, without
/// `course.view_all`, and unpublished content answers 404 to it (M1 spec 03 B13, B14).
async fn outline(org: &Org) -> (Uuid, Uuid, Uuid) {
    let course = org.course(org.tower, true).await;
    let module = org.module(course).await;
    let item = org.link(module).await;
    org.publish(module, item).await;
    (course, module, item)
}

/// [`outline`]'s course.
fn published_outline(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: vec![("course_id", outline(org).await.0)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// [`outline`]'s module.
fn published_module(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: vec![("module_id", outline(org).await.1)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// [`outline`]'s item.
fn published_item(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: vec![("item_id", outline(org).await.2)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// A published course in Tower and a new module's title. The content setups below use published
/// courses: a draft's modules and items answer 404 to a caller without `course.view_all` (M1 spec
/// 02 Q13), and the 2xx case holds `course.content_edit` alone.
fn new_module_in_tower_course(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: vec![("course_id", org.course(org.tower, true).await)],
            body: Some(serde_json::json!({ "title": "Suite module" })),
            at: At::Account(org.tower),
        }
    })
}

/// A course in Tower with two modules, put in the reverse order.
fn reverse_modules(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        let first = org.module(course).await;
        let second = org.module(course).await;
        Call {
            params: vec![("course_id", course)],
            body: Some(serde_json::json!({ "module_ids": [second, first] })),
            at: At::Account(org.tower),
        }
    })
}

/// A module of a course in Tower with a new title and rules, requiring the course's first
/// module. Its completion validity's cases are in `tests/completions.rs`: one more member would
/// pass [`MAX_MEMBERS`].
fn retitle_module(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        let first = org.module(course).await;
        Call {
            params: vec![("module_id", org.module(course).await)],
            body: Some(serde_json::json!({
                "title": "Retitled module",
                "published": true,
                "unlock_at": "2030-01-02T03:04:05Z",
                "prerequisite_ids": [first],
                "require_sequential": true,
                "completion_rule": "one",
            })),
            at: At::Account(org.tower),
        }
    })
}

/// An empty module of a course in Tower.
fn empty_module(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        Call {
            params: vec![("module_id", org.module(course).await)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// A module of a course in Tower and a new header.
fn new_header_in_module(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        Call {
            params: vec![("module_id", org.module(course).await)],
            body: Some(serde_json::json!({ "kind": "header", "title": "Suite header" })),
            at: At::Account(org.tower),
        }
    })
}

/// A module of a course in Tower with two items, put in the reverse order.
fn reverse_items(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        let module = org.module(course).await;
        let first = org.header(module).await;
        let second = org.header(module).await;
        Call {
            params: vec![("module_id", module)],
            body: Some(serde_json::json!({ "item_ids": [second, first] })),
            at: At::Account(org.tower),
        }
    })
}

/// An external link of a course in Tower with a new title and a requirement.
fn retitle_item(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        let module = org.module(course).await;
        Call {
            params: vec![("item_id", org.link(module).await)],
            body: Some(serde_json::json!({
                "title": "Retitled item",
                "published": true,
                "requirement": { "kind": "view" },
            })),
            at: At::Account(org.tower),
        }
    })
}

/// An item of a course in Tower moved to the course's second module.
fn move_item_to_second_module(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        let from = org.module(course).await;
        let to = org.module(course).await;
        Call {
            params: vec![("item_id", org.header(from).await)],
            body: Some(serde_json::json!({ "module_id": to, "position": 0 })),
            at: At::Account(org.tower),
        }
    })
}

/// An item of a course in Tower.
fn item_in_module(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        let module = org.module(course).await;
        Call {
            params: vec![("item_id", org.header(module).await)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// A `view` of a published external link with a `view` requirement, in a published module of
/// a published course in Tower; the caller holds their role in the course.
fn view_published_link(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        let module = org.module(course).await;
        let item = org.link(module).await;
        let publish = modules::ModulePatch {
            published: Some(true),
            ..modules::ModulePatch::default()
        };
        modules::update(&org.pool, scope(org.id), module, &publish, now())
            .await
            .expect("publish module");
        let publish = items::ItemPatch {
            published: Some(true),
            ..items::ItemPatch::default()
        };
        items::update(&org.pool, scope(org.id), item, &publish, now())
            .await
            .expect("publish item");
        let view = Some(stepclimb_core::rules::Requirement::View);
        items::set_requirement(&org.pool, scope(org.id), item, view, now())
            .await
            .expect("set requirement");
        Call {
            params: vec![("item_id", item)],
            body: Some(serde_json::json!({ "action": "view" })),
            at: At::Course(course),
        }
    })
}

/// A published course in Tower with one learner, a user enrolled with a custom role holding
/// only `course.participate`: the course and the learner as path parameters, for both staff
/// progress routes (the list takes no `user_id`, and ignores it). Held at Tower.
fn learner_of_tower_course(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        let enrollment = org.enrollment(course).await;
        let learner: Uuid = sqlx::query_scalar("SELECT user_id FROM enrollments WHERE id = $1")
            .bind(enrollment)
            .fetch_one(&org.pool)
            .await
            .expect("the learner");
        Call {
            params: vec![("course_id", course), ("user_id", learner)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// [`learner_of_tower_course`], held through an enrollment in the course that observes the
/// learner.
fn observed_learner_of_tower_course(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let call = learner_of_tower_course(org).await;
        let (course, learner) = (call.params[0].1, call.params[1].1);
        Call {
            at: At::Observing { course, learner },
            ..call
        }
    })
}

/// [`learner_of_tower_course`] with a completion of the course by the learner, written by SQL
/// as the roll-up writes it (M1 spec 14 B1), so the 2xx and cross-organization cases list a row.
fn completion_of_tower_learner(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let call = learner_of_tower_course(org).await;
        let (course, learner) = (call.params[0].1, call.params[1].1);
        sqlx::query(
            "INSERT INTO completions (id, org_id, user_id, course_id, target_type, target_id,
                                      cycle, completed_at)
             VALUES ($1, $2, $3, $4, 'course', $4, 1, now())",
        )
        .bind(new_id())
        .bind(org.id)
        .bind(learner)
        .bind(course)
        .execute(&org.pool)
        .await
        .expect("record a completion");
        call
    })
}

/// [`completion_of_tower_learner`], held through an enrollment in the course that observes the
/// learner.
fn observed_completion_of_tower_learner(
    org: &Org,
) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let call = completion_of_tower_learner(org).await;
        let (course, learner) = (call.params[0].1, call.params[1].1);
        Call {
            at: At::Observing { course, learner },
            ..call
        }
    })
}

/// A published course in Tower copied to Ground, inside Tower's subtree, so a grant at Tower
/// covers the source and the destination. Published, so the 403 case (every permission but
/// `course.view_all`) sees the course: a draft would answer 404 to it (M1 spec 02 Q13).
fn copy_course_to_ground(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let hex = new_id().simple().to_string();
        Call {
            params: vec![("course_id", org.course(org.tower, true).await)],
            body: Some(serde_json::json!({
                "account_id": org.ground, "code": format!("K{}", &hex[hex.len() - 12..]),
            })),
            at: At::Account(org.tower),
        }
    })
}

/// A course file that is not the organization's, or whose course the caller cannot see: 404
/// `file.not_found` (M1 spec 06 API).
const FILE_NOT_FOUND: Target = Target::Scoped {
    not_found: "file.not_found",
    cross_org: "file.not_found",
};

/// The query string a call of the operation carries, for the one route that takes its
/// arguments there: the upload's name (M1 spec 06 B3).
const QUERIES: &[(&str, &str)] = &[("uploadCourseFile", "name=suite.txt")];

/// A published course in Tower with a file in its top folder, listed. The file setups use
/// published courses: on a draft, the 403 case of a `course.view_all` route (every permission but
/// that one) and the 2xx case of a `course.content_edit` route (that one alone) would both get
/// 404 (M1 spec 02 Q13).
fn files_of_tower_course(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        org.course_file(course, "/").await;
        Call {
            params: vec![("course_id", course)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// A published course in Tower and a file's bytes; [`QUERIES`] names it.
fn upload_into_tower_course(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: vec![("course_id", org.course(org.tower, true).await)],
            body: Some(serde_json::json!("A suite file")),
            at: At::Account(org.tower),
        }
    })
}

/// A file of a published course in Tower.
fn course_file_in_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        Call {
            params: vec![("course_file_id", org.course_file(course, "/").await)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// A file of a published course in Tower, and a signed URL's body.
fn url_of_course_file_in_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        Call {
            params: vec![("course_file_id", org.course_file(course, "/").await)],
            body: Some(serde_json::json!({ "disposition": "inline" })),
            at: At::Account(org.tower),
        }
    })
}

/// A published `file` item in a published module of a published course in Tower, with no unlock
/// rule, and a signed URL's body. The 2xx case holds `course.view` alone.
fn url_of_published_file_item(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        let module = org.module(course).await;
        let item = org
            .file_item(module, org.course_file(course, "/").await)
            .await;
        org.publish(module, item).await;
        Call {
            params: vec![("item_id", item)],
            body: Some(serde_json::json!({ "disposition": "attachment" })),
            at: At::Account(org.tower),
        }
    })
}

/// A file of a published course in Tower, renamed and moved.
fn rename_course_file(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        Call {
            params: vec![("course_file_id", org.course_file(course, "/").await)],
            body: Some(serde_json::json!({ "folder": "/moved", "name": "renamed.txt" })),
            at: At::Account(org.tower),
        }
    })
}

/// A published course in Tower whose folder `/a` holds a file, renamed to `/b`.
fn rename_tower_folder(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        org.course_file(course, "/a").await;
        Call {
            params: vec![("course_id", course)],
            body: Some(serde_json::json!({ "from": "/a", "to": "/b" })),
            at: At::Account(org.tower),
        }
    })
}

/// A page that is not the organization's, or whose course the caller cannot see: 404
/// `page.not_found` (M1 spec 07 API).
const PAGE_NOT_FOUND: Target = Target::Scoped {
    not_found: "page.not_found",
    cross_org: "page.not_found",
};

/// A published course in Tower with a page, listed. Published for the reason
/// [`files_of_tower_course`] gives.
fn pages_of_tower_course(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        org.page(course).await;
        Call {
            params: vec![("course_id", course)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// A video upload that is not the organization's, or whose course the caller cannot see: 404
/// `video_upload.not_found` (M1 spec 08 API).
const VIDEO_UPLOAD_NOT_FOUND: Target = Target::Scoped {
    not_found: "video_upload.not_found",
    cross_org: "video_upload.not_found",
};

/// A video that is not the organization's, or none of whose courses the caller can see: 404
/// `video.not_found` (M1 spec 08 API).
const VIDEO_NOT_FOUND: Target = Target::Scoped {
    not_found: "video.not_found",
    cross_org: "video.not_found",
};

/// The headers a call of the operation carries besides the suite's own: tus's (M1 spec 08 B4,
/// B5). A `Content-Type` here replaces the JSON one, and the body is sent as its JSON text.
const HEADERS: &[(&str, &[(&str, &str)])] = &[
    (
        "createVideoUpload",
        &[
            ("tus-resumable", "1.0.0"),
            ("upload-length", "64"),
            ("upload-metadata", "title U3VpdGUgdmlkZW8="),
        ],
    ),
    ("getVideoUploadOffset", &[("tus-resumable", "1.0.0")]),
    (
        "appendVideoUpload",
        &[
            ("tus-resumable", "1.0.0"),
            ("upload-offset", "0"),
            ("content-type", "application/offset+octet-stream"),
        ],
    ),
    ("deleteVideoUpload", &[("tus-resumable", "1.0.0")]),
];

/// A published course in Tower with an upload started in it, listed. The video setups use
/// published courses for the reason the file setups do.
fn videos_of_tower_course(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        org.video_upload(course).await;
        Call {
            params: vec![("course_id", course)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// A new page of a published course in Tower.
fn new_page_in_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: vec![("course_id", org.course(org.tower, true).await)],
            body: Some(serde_json::json!({ "title": "Suite page" })),
            at: At::Account(org.tower),
        }
    })
}

/// A page of a published course in Tower.
fn page_in_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        Call {
            params: vec![("page_id", org.page(course).await.0)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// A published course in Tower; [`HEADERS`] start an upload in it.
fn tower_course(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: vec![("course_id", org.course(org.tower, true).await)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// A page of a published course in Tower, retitled and given a new slug.
fn retitle_page(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        Call {
            params: vec![("page_id", org.page(course).await.0)],
            body: Some(serde_json::json!({ "title": "Retitled", "slug": "retitled" })),
            at: At::Account(org.tower),
        }
    })
}

/// A new body for a page of a published course in Tower, from its current version.
fn save_page_body(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        let (page, version) = org.page(course).await;
        let body = serde_json::json!({ "type": "doc", "content": [
            { "type": "paragraph", "content": [{ "type": "text", "text": "Saved" }] },
        ] });
        Call {
            params: vec![("page_id", page)],
            body: Some(serde_json::json!({ "body": body, "base_version_id": version })),
            at: At::Account(org.tower),
        }
    })
}

/// Version 1 of a page of a published course in Tower.
fn page_version_in_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        let (page, version) = org.page(course).await;
        Call {
            params: vec![("page_id", page), ("version_id", version)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// An upload of 15 bytes started in a published course in Tower.
fn video_upload_in_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        Call {
            params: vec![("upload_id", org.video_upload(course).await.0)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// Version 1 of a page of a published course in Tower restored over version 2.
fn restore_page_version(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        let (page, first) = org.page(course).await;
        let second = org.save_page(page, first).await;
        Call {
            params: vec![("page_id", page), ("version_id", first)],
            body: Some(serde_json::json!({ "base_version_id": second })),
            at: At::Account(org.tower),
        }
    })
}

/// The chunk that completes an upload in a published course in Tower: the JSON text of a
/// string, the upload's 15 bytes.
fn chunk_of_video_upload_in_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        Call {
            params: vec![("upload_id", org.video_upload(course).await.0)],
            body: Some(serde_json::json!("A suite chunk")),
            at: At::Account(org.tower),
        }
    })
}

/// The video of an upload in a published course in Tower.
fn video_in_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        Call {
            params: vec![("video_id", org.video_upload(course).await.1)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// The video of an upload in a published course in Tower, renamed.
fn rename_video_in_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        Call {
            params: vec![("video_id", org.video_upload(course).await.1)],
            body: Some(serde_json::json!({ "title": "Renamed" })),
            at: At::Account(org.tower),
        }
    })
}

/// The video of a complete upload in a published course in Tower whose transcode failed, so it
/// can be retried (M1 spec 08 B2).
fn failed_video_in_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let course = org.course(org.tower, true).await;
        let (upload, video) = org.video_upload(course).await;
        sqlx::query("UPDATE video_uploads SET received = length WHERE id = $1")
            .bind(upload)
            .execute(&org.pool)
            .await
            .expect("complete the upload");
        sqlx::query(
            "UPDATE videos SET status = 'failed', error_code = 'video.transcode_failed'
             WHERE id = $1",
        )
        .bind(video)
        .execute(&org.pool)
        .await
        .expect("fail the video");
        Call {
            params: vec![("video_id", video)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// A role that is not the organization's: 404 `role.not_found`. Roles are organization-wide,
/// so a role route's scope is the organization and it has no wrong-account 404 case.
const ROLE_NOT_FOUND: Target = Target::Scoped {
    not_found: "role.not_found",
    cross_org: "role.not_found",
};

/// A new unassigned custom role of `org`, named in the path; the caller acts at organization
/// scope.
fn custom_role_in_path(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: vec![("role_id", org.custom_role().await)],
            body: None,
            at: At::Org,
        }
    })
}

/// [`custom_role_in_path`] with a new name in the body.
fn rename_custom_role(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            body: Some(serde_json::json!({ "name": "Renamed role" })),
            ..custom_role_in_path(org).await
        }
    })
}

/// A role with no permissions, which any `role.manage` holder may define.
fn new_role_in_body(_: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({
                "key": "suite_new_role",
                "name": "Suite new role",
                "permissions": [],
            })),
            at: At::Org,
        }
    })
}

/// A protected operation and how to call it.
struct Entry {
    /// The OpenAPI `operationId`; the method and path come from the document.
    operation_id: &'static str,
    access: Access,
}

/// What an operation needs besides a principal.
enum Access {
    /// A principal and no permission (`GET /api/v1/me`): the 401, 2xx and cross-organization
    /// cases.
    AuthOnly(Setup),
    /// A principal and no permission, on a row of the caller's own that the setup builds for
    /// the user it is given: the 401, 2xx, other-user and cross-organization cases. Another
    /// user's row, in the same organization or another, answers 404 with `not_found`.
    Own {
        not_found: &'static str,
        setup: OwnSetup,
    },
    /// A principal and no permission, with the setup building the caller's own rows the
    /// request needs but does not name: the 401, 2xx, every-feature-off and cross-organization
    /// cases.
    Mine(MineSetup),
    /// `Require<P, T>`: every case.
    Permission {
        /// `P::KEY`.
        key: &'static str,
        target: Target,
        /// Permissions the 2xx case adds to `P`, which the route documents (a move's
        /// destination).
        extra: &'static [&'static str],
        /// `F::KEY` of the `Require<F>` the route takes before the permission, if any.
        feature: Option<&'static str>,
        /// Whether the route checks `feature` only once it has found its target, because the
        /// target decides which feature applies (a provider's kind, spec 06 I5): its "feature
        /// off, unknown ids" case then answers the target's 404.
        feature_by_target: bool,
        /// Permissions the 403 case leaves out besides `P`: the other permissions the handler
        /// grants the route on, when it decides between several (M1 spec 05 B20, T14a).
        denied_also: &'static [&'static str],
        setup: Setup,
    },
}

/// How the route finds its target, as far as the cases need to know.
#[derive(Clone, Copy)]
enum Target {
    /// `OrgTarget`: the caller's own organization. No 404 case; the cross-organization case acts
    /// on org A and answers 2xx.
    Org,
    /// `AnyScope` without a target (a list). No 404 case; the cross-organization case answers
    /// 2xx.
    AnyScope,
    /// A target the setup builds, in the path (`AccountTarget`, `AccountParentTarget`) or the
    /// body (`AnyScope` with `require_at`). Hidden, it answers 404 with `not_found`; built in
    /// another organization, 404 with `cross_org`, which differs when the route checks another
    /// id of the request first (a role assignment's user, B12).
    Scoped {
        not_found: &'static str,
        cross_org: &'static str,
    },
}

/// Builds what one case calls in a fresh organization.
type Setup = for<'a> fn(&'a Org) -> Pin<Box<dyn Future<Output = Call> + Send + 'a>>;

/// Builds what one case calls in a fresh organization, on a row of the user it is given.
type OwnSetup = for<'a> fn(&'a Org, Uuid) -> Pin<Box<dyn Future<Output = Call> + Send + 'a>>;

/// Builds what one case calls in a fresh organization, for the user it is given, who makes
/// the call.
type MineSetup = for<'a> fn(&'a Org, Uuid) -> Pin<Box<dyn Future<Output = Mine> + Send + 'a>>;

/// A call a [`MineSetup`] built, and the flow cookie it sends beside the caller's session.
struct Mine {
    call: Call,
    flow: Option<String>,
}

impl Mine {
    fn call(call: Call) -> Self {
        Self { call, flow: None }
    }

    /// The `Cookie` value: `session` (when there is one) and the flow cookie.
    fn cookie(&self, session: Option<&str>) -> Option<String> {
        match (session, &self.flow) {
            (Some(session), Some(flow)) => Some(format!("{session}; {flow}")),
            (Some(session), None) => Some(session.to_owned()),
            (None, flow) => flow.clone(),
        }
    }
}

/// One call of an operation.
struct Call {
    /// Values of the path's `{name}` segments.
    params: Vec<(&'static str, Uuid)>,
    body: Option<Value>,
    /// The target's scope, where the 403 and 2xx cases assign their role: the organization for
    /// `OrgTarget`, the parent account for `AccountParentTarget`, the body's scope for a body
    /// target.
    at: At,
}

#[derive(Clone, Copy, Debug)]
enum At {
    Org,
    Account(Uuid),
    /// A course, where a role is held through an active enrollment (M1 spec 02 Q1): the only
    /// way to hold a course-only permission such as `course.participate`.
    Course(Uuid),
    /// [`At::Course`], the enrollment also naming `learner` as observed (M1 spec 05 B17), by
    /// SQL whatever the role holds, so a holder without `progress.view_observed` keeps a link
    /// that must grant nothing.
    Observing {
        course: Uuid,
        learner: Uuid,
    },
}

// The walker (B21, T16).

const METHODS: [&str; 7] = ["get", "head", "options", "post", "put", "patch", "delete"];

/// An operation of the document.
#[derive(Clone, Debug)]
struct Operation {
    id: String,
    method: Method,
    /// The path template, `{name}` for a parameter.
    path: String,
}

fn document() -> Value {
    serde_json::to_value(openapi::document()).expect("the document serializes")
}

/// Every operation of `doc`, or the `method path` of those without an `operationId`.
fn operations(doc: &Value) -> Result<Vec<Operation>, Vec<String>> {
    let mut found = Vec::new();
    let mut unnamed = Vec::new();
    for (path, item) in doc["paths"].as_object().expect("paths") {
        for (method, operation) in item.as_object().expect("a path item") {
            if !METHODS.contains(&method.as_str()) {
                continue;
            }
            match operation["operationId"].as_str() {
                Some(id) => found.push(Operation {
                    id: id.to_owned(),
                    method: Method::from_bytes(method.to_uppercase().as_bytes()).expect("a method"),
                    path: path.clone(),
                }),
                None => unnamed.push(format!("{method} {path}")),
            }
        }
    }
    if unnamed.is_empty() {
        Ok(found)
    } else {
        Err(unnamed)
    }
}

/// What is wrong between `doc` and the suite's lists: operations in neither list, list names
/// that are no operation, and names listed twice.
fn walk(doc: &Value, table: &[&str], public: &[&str]) -> Vec<String> {
    let operations = match operations(doc) {
        Ok(operations) => operations,
        Err(unnamed) => return vec![format!("operations without an operationId: {unnamed:?}")],
    };
    let mut problems = Vec::new();
    for op in &operations {
        if !table.contains(&op.id.as_str()) && !public.contains(&op.id.as_str()) {
            problems.push(format!(
                "{} ({} {}) has no permission suite entry and is not public: add it to TABLE \
                 in tests/permissions.rs",
                op.id, op.method, op.path
            ));
        }
    }
    let listed: Vec<&str> = table.iter().chain(public).copied().collect();
    for (i, name) in listed.iter().enumerate() {
        if !operations.iter().any(|op| op.id == *name) {
            problems.push(format!(
                "{name} is listed but is no operation of the document"
            ));
        }
        if listed[..i].contains(name) {
            problems.push(format!("{name} is listed twice"));
        }
    }
    problems
}

/// Each operation of [`TABLE`] once, in table order.
fn table_ids() -> Vec<&'static str> {
    let mut ids: Vec<&'static str> = Vec::new();
    for entry in TABLE {
        if !ids.contains(&entry.operation_id) {
            ids.push(entry.operation_id);
        }
    }
    ids
}

/// The operations whose handler grants them on more than one permission, each with one entry
/// per granting permission (M1 spec 05 B20, T14a; spec 14 T10a).
const PER_GRANT: &[&str] = &[
    "listCourseProgress",
    "getLearnerProgress",
    "listCourseCompletions",
];

/// An operation has several entries only when it is in [`PER_GRANT`], and then each is a
/// permission entry naming a different permission.
#[test]
fn only_per_grant_operations_have_several_entries_each_with_its_own_permission() {
    for id in table_ids() {
        let entries: Vec<&Entry> = TABLE.iter().filter(|e| e.operation_id == id).collect();
        if entries.len() == 1 {
            assert!(
                !PER_GRANT.contains(&id),
                "{id} has one entry but is in PER_GRANT"
            );
            continue;
        }
        assert!(
            PER_GRANT.contains(&id),
            "{id} is listed {} times",
            entries.len()
        );
        let mut keys: Vec<&str> = entries
            .iter()
            .map(|e| match e.access {
                Access::Permission { key, .. } => key,
                _ => panic!("{id}: a per-grant entry names a permission"),
            })
            .collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), entries.len(), "{id} repeats a permission");
    }
}

#[test]
fn b21_every_operation_is_in_the_table_or_the_public_list() {
    let problems = walk(&document(), &table_ids(), PUBLIC);
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn t16_an_operation_without_an_entry_fails_the_walk_by_name() {
    let mut doc = document();
    doc["paths"]["/api/v1/widgets"]["post"] = serde_json::json!({ "operationId": "createWidget" });
    let problems = walk(&doc, &table_ids(), PUBLIC);
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert!(
        problems[0].starts_with("createWidget (POST /api/v1/widgets)"),
        "{problems:#?}"
    );
}

#[test]
fn the_walk_refuses_stale_duplicate_and_unnamed_entries() {
    let doc = document();
    let problems = walk(&doc, &["healthz", "removedRoute"], PUBLIC);
    assert!(
        problems.contains(&"removedRoute is listed but is no operation of the document".into()),
        "{problems:#?}"
    );
    assert!(
        problems.contains(&"healthz is listed twice".into()),
        "{problems:#?}"
    );
    let mut unnamed = doc;
    unnamed["paths"]["/api/v1/widgets"]["get"] = serde_json::json!({});
    assert_eq!(
        walk(&unnamed, &table_ids(), PUBLIC),
        ["operations without an operationId: [\"get /api/v1/widgets\"]"]
    );
}

/// The code of a disabled feature, which an operation's 403 documents when its route takes
/// `Require<F>` (spec 10 API).
const FEATURE_UNAVAILABLE: &str = "`entitlement.feature_unavailable`";

/// The operations whose 403 documents [`FEATURE_UNAVAILABLE`] without a feature in their
/// entry, and those with a feature in their entry whose 403 does not document it.
fn feature_doc_mismatches(doc: &Value, table: &[Entry]) -> Vec<String> {
    let operations = operations(doc).expect("named operations");
    let mut problems = Vec::new();
    for op in &operations {
        let gated = table.iter().any(|entry| {
            entry.operation_id == op.id
                && matches!(
                    entry.access,
                    Access::Permission {
                        feature: Some(_),
                        ..
                    }
                )
        });
        let mut forbidden =
            &doc["paths"][&op.path][op.method.as_str().to_lowercase()]["responses"]["403"];
        if let Some(reference) = forbidden["$ref"].as_str() {
            forbidden = doc
                .pointer(reference.trim_start_matches('#'))
                .unwrap_or(&Value::Null);
        }
        let documented = forbidden["description"]
            .as_str()
            .is_some_and(|text| text.contains(FEATURE_UNAVAILABLE));
        match (gated, documented) {
            (true, false) => problems.push(format!(
                "{}: its entry names a feature but its 403 does not document {FEATURE_UNAVAILABLE}",
                op.id
            )),
            (false, true) => problems.push(format!(
                "{}: its 403 documents {FEATURE_UNAVAILABLE} but its entry names no feature",
                op.id
            )),
            _ => {}
        }
    }
    problems
}

#[test]
fn a_403_documents_the_feature_code_exactly_when_the_entry_names_a_feature() {
    let doc = document();
    let problems = feature_doc_mismatches(&doc, TABLE);
    assert!(problems.is_empty(), "{problems:#?}");

    let mut undocumented = doc.clone();
    undocumented["components"]["responses"]["DeleteForbidden"]["description"] =
        Value::String("`permission.denied`: no.".into());
    assert_eq!(
        feature_doc_mismatches(&undocumented, TABLE),
        [format!(
            "deleteRole: its entry names a feature but its 403 does not document \
             {FEATURE_UNAVAILABLE}"
        )]
    );

    let mut overdocumented = doc;
    overdocumented["paths"]["/api/v1/admin/roles"]["get"]["responses"]["403"] =
        serde_json::json!({ "description": FEATURE_UNAVAILABLE });
    assert_eq!(
        feature_doc_mismatches(&overdocumented, TABLE),
        [format!(
            "listRoles: its 403 documents {FEATURE_UNAVAILABLE} but its entry names no feature"
        )]
    );
}

/// A protected route put in the public list would skip its cases; this catches it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn public_operations_answer_without_a_credential(pool: PgPool) {
    let world = World::new(pool).await;
    let app = router(world.state()).layer(Extension(ConnectInfo(CLIENT)));
    let org = world.org().await;
    let operations = operations(&document()).expect("named operations");
    for id in PUBLIC {
        let op = operations
            .iter()
            .find(|op| op.id == *id)
            .expect("listed operations exist");
        if *id == "startProviderSignIn" {
            provider_sign_in_answers_without_a_credential(&world, &app, &org).await;
            continue;
        }
        if *id == "finishProviderSignIn" {
            provider_callback_answers_without_a_credential(&app, &org).await;
            continue;
        }
        if *id == "authorizeApp" || *id == "answerAppAuthorization" {
            authorization_answers_without_a_credential(&app, &org).await;
            continue;
        }
        if *id == "requestAppToken" || *id == "revokeAppToken" {
            token_endpoint_answers_without_a_credential(&app, &org, &op.path).await;
            continue;
        }
        if *id == "getSignedFile" {
            signed_file_answers_without_a_credential(&world, &app, &org).await;
            continue;
        }
        let (body, cookie) = public_request(&world, &org, id).await;
        let answer = send(
            &app,
            &op.method,
            &op.path,
            org.id,
            cookie.as_deref(),
            body.as_ref(),
        )
        .await;
        assert!(answer.status.is_success(), "{id}: {answer:?}");
    }
}

/// A signed file URL serves its file to a request with no session at all: the token is the
/// credential (M1 spec 06 B17).
async fn signed_file_answers_without_a_credential(world: &World, app: &Router, org: &Org) {
    let state = world.state();
    let storage = state.storage.clone().expect("the suite's store");
    let bytes = format!("File of {}", org.slug).into_bytes();
    let mut hasher = files::Sha256Hasher::new();
    hasher.update(&bytes);
    let hashed = hasher.finish();
    let sha256 = hashed.sha256();
    let new = files::NewFile {
        hashed,
        mime: "text/plain; charset=utf-8".into(),
    };
    let file = files::insert(&world.pool, scope(org.id), &new, now())
        .await
        .expect("insert file");
    storage
        .store()
        .put(
            &storage.file_path(scope(org.id), sha256),
            bytes.clone().into(),
        )
        .await
        .expect("store the bytes");
    let course = org.course(org.tower, true).await;
    let place = (
        &course_files::Folder::root(),
        &course_files::Name::parse("notes.txt").expect("a name"),
    );
    let mut conn = world.pool.acquire().await.expect("a connection");
    let course_file = course_files::create(&mut conn, scope(org.id), course, file.id, place, now())
        .await
        .expect("create course file")
        .id;
    let grant = stepclimb_server::file_urls::Grant {
        org_id: org.id,
        file_id: file.id,
        course_file_id: course_file,
        disposition: stepclimb_server::file_urls::Disposition::Attachment,
        expires_at: now()
            .duration_since(UNIX_EPOCH)
            .expect("after the epoch")
            .as_secs()
            + 60,
    };
    let token = stepclimb_server::file_urls::seal(&state.keys, &grant).expect("seal");
    let answer = send(
        app,
        &Method::GET,
        &format!("/files/{token}"),
        org.id,
        None,
        None,
    )
    .await;
    assert!(answer.status.is_success(), "getSignedFile: {answer:?}");
    assert_eq!(answer.body.as_bytes(), bytes, "getSignedFile");
}

/// The OAuth 2.1 authorization endpoint answers an anonymous browser without a 401 problem: a
/// good request goes to sign-in carrying it, and an answer to the confirmation page shows the
/// endpoint's own error page, since a browser navigation would show a problem body raw.
async fn authorization_answers_without_a_credential(app: &Router, org: &Org) {
    let request = "/oauth/authorize?response_type=code&client_id=stepclimb-app\
                   &redirect_uri=http%3A%2F%2F127.0.0.1%3A43210%2Foauth%2Fcallback\
                   &code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM\
                   &code_challenge_method=S256";
    let opened = send(app, &Method::GET, request, org.id, None, None).await;
    assert_eq!(opened.status, StatusCode::SEE_OTHER, "{opened:?}");
    let location = opened.headers[header::LOCATION].to_str().expect("ASCII");
    assert!(location.starts_with("/sign-in?return_to="), "{location}");
    let answered = send(app, &Method::POST, "/oauth/authorize", org.id, None, None).await;
    assert_eq!(answered.status, StatusCode::BAD_REQUEST, "{answered:?}");
    assert!(
        answered.body.contains("<code>request.malformed</code>"),
        "{answered:?}"
    );
}

/// The token and revocation endpoints answer a form with no credential: revoking an unknown token
/// is 200 (RFC 7009), and an unknown refresh token is the RFC 6749 `invalid_grant`, never a 401.
async fn token_endpoint_answers_without_a_credential(app: &Router, org: &Org, path: &str) {
    let body = if path == "/oauth/revoke" {
        "token=unknown&client_id=stepclimb-app"
    } else {
        "grant_type=refresh_token&refresh_token=unknown&client_id=stepclimb-app"
    };
    let request = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(TEST_ORG_HEADER, org.id.to_string())
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .expect("a valid request");
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("the router never fails");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("the body is readable")
        .to_bytes();
    let text = String::from_utf8_lossy(&bytes);
    if path == "/oauth/revoke" {
        assert_eq!(status, StatusCode::OK, "{path}: {text}");
    } else {
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}: {text}");
        assert!(text.contains("\"invalid_grant\""), "{text}");
    }
}

/// The provider sign-in start answers a redirect, not a 2xx: with an enabled provider of the
/// organization pointing at the mock IdP, an anonymous request is sent to the provider's
/// authorization endpoint (a protected route would answer 401 instead).
async fn provider_sign_in_answers_without_a_credential(world: &World, app: &Router, org: &Org) {
    let mock = stepclimb_mock_idp::MockIdp::start()
        .await
        .expect("the mock starts");
    let id = new_id();
    let config = serde_json::json!({
        "issuer": mock.issuer(),
        "client_id": mock.client_id(),
        "client_secret": mock.client_secret(),
    });
    let stored = stepclimb_auth::oidc_config::validate(
        &config,
        stepclimb_auth::outbound::UrlPolicy::new(true, true),
        None,
    )
    .expect("a valid config")
    .seal(
        &world.state().keys,
        stepclimb_auth::oidc_config::ProviderRow {
            org_id: org.id,
            provider_id: id,
        },
    )
    .expect("seal")
    .to_json();
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config) \
         VALUES ($1, $2, 'mock', 'oidc', $3)",
    )
    .bind(id)
    .bind(org.id)
    .bind(Value::Object(stored))
    .execute(&world.pool)
    .await
    .expect("insert provider");
    let answer = send(app, &Method::GET, "/auth/mock/start", org.id, None, None).await;
    assert_eq!(answer.status, StatusCode::SEE_OTHER, "{answer:?}");
    let location = answer.headers[header::LOCATION].to_str().expect("ASCII");
    let authorize = mock.url(stepclimb_mock_idp::Endpoint::Authorize);
    assert!(location.starts_with(&authorize), "{location}");
}

/// The provider callback answers a redirect, not a 2xx: without a flow cookie an anonymous
/// request lands on the sign-in page with `auth.flow_invalid` (a protected route would answer
/// 401). The provider the start case inserted is the organization's `mock`.
async fn provider_callback_answers_without_a_credential(app: &Router, org: &Org) {
    let path = "/auth/mock/callback?code=c&state=s";
    let answer = send(app, &Method::GET, path, org.id, None, None).await;
    assert_eq!(answer.status, StatusCode::SEE_OTHER, "{answer:?}");
    assert_eq!(
        answer.headers[header::LOCATION],
        "/sign-in?error=auth.flow_invalid"
    );
}

/// The body and cookie a public operation needs to succeed: the enrollment start route takes a
/// live enrollment link of the organization's user, and the passkey route that link, a started
/// registration's flow cookie and a software passkey's answer to its options; the others take
/// none.
async fn public_request(world: &World, org: &Org, id: &str) -> (Option<Value>, Option<String>) {
    match id {
        "startPasskeySignIn" => {
            return (
                Some(serde_json::json!({ "email": "anyone@example.test" })),
                None,
            );
        }
        "finishPasskeySignIn" => return passkey_sign_in_request(world, org).await,
        "signInWithPassword" => return password_sign_in_request(org).await,
        "signInWithTotp" => return totp_sign_in_request(org).await,
        "enrollTotpAtSignIn" => return totp_set_up_request(org).await,
        "startEnrollment" | "enrollPasskey" | "enrollPassword" => {}
        _ => return (None, None),
    }
    let (_, user) = org
        .named_user("Pat Example".into(), Some(format!("{id}@example.test")))
        .await;
    let link = stepclimb_server::enrollment::create_link(
        &world.pool,
        scope(org.id),
        user,
        stepclimb_core::local_tokens::Origin::Bootstrap,
        &world.state().public_url,
        now(),
        Actor::Instance,
    )
    .await
    .expect("create an enrollment link");
    let (_, token) = link.expose_url().split_once("#token=").expect("a token");
    if id == "startEnrollment" {
        return (Some(serde_json::json!({ "token": token })), None);
    }
    if id == "enrollPassword" {
        let body =
            serde_json::json!({ "token": token, "password": "correct horse battery staple" });
        return (Some(body), None);
    }
    let mut conn = world.pool.acquire().await.expect("a connection");
    let hash = stepclimb_auth::token::TokenHash::of(token);
    let enrollment = stepclimb_core::local_tokens::check_enrollment(
        &mut conn,
        scope(org.id),
        hash.as_bytes(),
        now(),
    )
    .await
    .expect("a live link");
    let begun = stepclimb_server::passkey_enrollment::begin(
        &world.state(),
        &mut conn,
        scope(org.id),
        &enrollment,
        now(),
    )
    .await
    .expect("start a registration");
    let options: webauthn_rs::prelude::CreationChallengeResponse =
        serde_json::from_value(serde_json::to_value(&begun.options).expect("JSON"))
            .expect("creation options");
    let origin = webauthn_rs::prelude::Url::parse("https://lms.example.org").expect("a URL");
    let credential = webauthn_authenticator_rs::softpasskey::SoftPasskey::new(true)
        .do_registration(origin, options)
        .expect("the soft passkey registers");
    let cookie = begun.set_cookie.to_str().expect("ASCII");
    let cookie = cookie.split(';').next().expect("a pair").to_owned();
    let body = serde_json::json!({ "token": token, "credential": credential });
    (Some(body), Some(cookie))
}

/// A user with a password, and the body that signs in with it.
async fn password_sign_in_request(org: &Org) -> (Option<Value>, Option<String>) {
    let email = "password@example.test";
    let (_, user) = org
        .named_user("Pat Example".into(), Some(email.into()))
        .await;
    let hash = stepclimb_auth::password::hash("correct horse battery staple").expect("a hash");
    stepclimb_core::local_logins::set_password(
        &org.pool,
        scope(org.id),
        user,
        hash.as_str(),
        now(),
        Actor::Instance,
    )
    .await
    .expect("set the password");
    let body = serde_json::json!({ "email": email, "password": "correct horse battery staple" });
    (Some(body), None)
}

/// A user with a password and confirmed TOTP, the second step started for them, and the
/// current code: the body and flow cookie `signInWithTotp` signs in with.
async fn totp_sign_in_request(org: &Org) -> (Option<Value>, Option<String>) {
    let (_, user) = org
        .named_user("Pat Example".into(), Some("totp@example.test".into()))
        .await;
    let secret = give_totp(org, user, true).await;
    let keys = state_for(&org.pool).keys;
    let flow = stepclimb_server::totp::begin_second_step(
        &org.pool,
        &keys,
        scope(org.id),
        user,
        1,
        None,
        now(),
    )
    .await
    .expect("start the second step");
    let cookie = flow.set_cookie.to_str().expect("ASCII");
    let cookie = cookie.split(';').next().expect("a pair").to_owned();
    let body = serde_json::json!({ "code": secret.code_at_time(now()) });
    (Some(body), Some(cookie))
}

/// A user with a password and a waiting TOTP secret, the set-up flow a password sign-in starts
/// while TOTP is required, and the current code: the body and flow cookie `enrollTotpAtSignIn`
/// signs in with.
async fn totp_set_up_request(org: &Org) -> (Option<Value>, Option<String>) {
    let (_, user) = org
        .named_user("Pat Example".into(), Some("set-up@example.test".into()))
        .await;
    let secret = give_totp(org, user, false).await;
    let keys = state_for(&org.pool).keys;
    let sealed: Vec<u8> =
        sqlx::query_scalar("SELECT totp_secret FROM local_logins WHERE user_id = $1")
            .bind(user)
            .fetch_one(&org.pool)
            .await
            .expect("the waiting secret");
    let pending = stepclimb_server::totp_enrollment::SetUpPending {
        generation: 1,
        secret_hash: stepclimb_server::totp_enrollment::sealed_secret_hash(&sealed),
    };
    let payload = serde_json::to_vec(&pending).expect("JSON");
    let start = stepclimb_server::flow::FlowStart {
        kind: stepclimb_core::auth_flows::Kind::PasswordTotpEnroll,
        user_id: Some(user),
        provider_id: None,
        payload: &payload,
        return_to: None,
    };
    let flow = stepclimb_server::flow::start(&org.pool, &keys, scope(org.id), start, now())
        .await
        .expect("start the set-up");
    let cookie = flow.set_cookie.to_str().expect("ASCII");
    let cookie = cookie.split(';').next().expect("a pair").to_owned();
    let body = serde_json::json!({ "code": secret.code_at_time(now()) });
    (Some(body), Some(cookie))
}

/// A user with a software passkey, a started sign-in for them, and the passkey's assertion:
/// the body and flow cookie `finishPasskeySignIn` signs in with.
async fn passkey_sign_in_request(world: &World, org: &Org) -> (Option<Value>, Option<String>) {
    use stepclimb_auth::passkey::{Registrant, RelyingParty};
    use webauthn_rs::prelude::{CreationChallengeResponse, RequestChallengeResponse, Url};
    let email = "signs-in@example.test";
    let (_, user) = org
        .named_user("Pat Example".into(), Some(email.into()))
        .await;
    // A local account, as enrollment leaves: what lets the passkey sign in (spec 04 Q16).
    org.local_identity(user).await;
    let origin = Url::parse("https://lms.example.org").expect("a URL");
    let party = RelyingParty::new(origin.as_str(), "Org").expect("a relying party");
    let registrant = Registrant {
        user_id: user,
        name: email,
        display_name: "Pat Example",
    };
    let started = party
        .start_registration(registrant, Vec::new())
        .expect("start a registration");
    let options: CreationChallengeResponse =
        serde_json::from_value(started.options).expect("creation options");
    let mut authenticator = webauthn_authenticator_rs::softpasskey::SoftPasskey::new(true);
    let response = authenticator
        .do_registration(origin.clone(), options)
        .expect("the soft passkey registers");
    let response = serde_json::to_value(response).expect("JSON");
    let registered = party
        .finish_registration(&response, &started.state)
        .expect("the registration verifies");
    let new = stepclimb_core::passkeys::NewPasskey {
        user_id: user,
        credential_id: &registered.credential_id,
        credential: &registered.credential,
        name: "Phone",
    };
    stepclimb_core::passkeys::create(&world.pool, scope(org.id), new, now(), Actor::Instance)
        .await
        .expect("store the passkey");
    let mut conn = world.pool.acquire().await.expect("a connection");
    let request: stepclimb_server::passkey_sign_in::PasskeySignInStart =
        serde_json::from_value(serde_json::json!({ "email": email })).expect("a start");
    let begun = stepclimb_server::passkey_sign_in::begin(
        &world.state(),
        &mut conn,
        scope(org.id),
        &request,
        now(),
    )
    .await
    .expect("start a sign-in");
    let options: RequestChallengeResponse =
        serde_json::from_value(serde_json::to_value(&begun.options).expect("JSON"))
            .expect("request options");
    let assertion = authenticator
        .do_authentication(origin, options)
        .expect("the soft passkey signs");
    let cookie = begun.set_cookie.to_str().expect("ASCII");
    let cookie = cookie.split(';').next().expect("a pair").to_owned();
    let body = serde_json::json!({ "credential": assertion });
    (Some(body), Some(cookie))
}

// The suite over the real router.

/// How many tests [`TABLE`] is run in. Entry `i` runs in shard `i % SHARDS`, so nextest runs
/// the shards in parallel, each in a database of its own, and a longer table makes each a little
/// longer rather than one test long.
const SHARDS: usize = 8;

/// Runs every entry of shard `shard` and fails naming each failing case.
async fn b20_shard(pool: PgPool, shard: usize) {
    let world = World::new(pool).await;
    let app = router(world.state()).layer(Extension(ConnectInfo(CLIENT)));
    let operations = operations(&document()).expect("named operations");
    let mut failures = Vec::new();
    for entry in TABLE.iter().skip(shard).step_by(SHARDS) {
        let op = operations
            .iter()
            .find(|op| op.id == entry.operation_id)
            .unwrap_or_else(|| panic!("{} is not in the document", entry.operation_id));
        failures.extend(run(&world, &app, op, &entry.access).await.failures);
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// One `#[sqlx::test]` per shard, and [`B20_SHARDS`], the shards they run.
macro_rules! b20_shards {
    ($($name:ident = $shard:literal),* $(,)?) => {
        const B20_SHARDS: &[usize] = &[$($shard),*];
        $(
            #[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
            async fn $name(pool: PgPool) {
                b20_shard(pool, $shard).await;
            }
        )*
    };
}

b20_shards!(
    b20_table_entries_pass_their_cases_shard_0 = 0,
    b20_table_entries_pass_their_cases_shard_1 = 1,
    b20_table_entries_pass_their_cases_shard_2 = 2,
    b20_table_entries_pass_their_cases_shard_3 = 3,
    b20_table_entries_pass_their_cases_shard_4 = 4,
    b20_table_entries_pass_their_cases_shard_5 = 5,
    b20_table_entries_pass_their_cases_shard_6 = 6,
    b20_table_entries_pass_their_cases_shard_7 = 7,
);

/// The shard tests run every shard once, so every entry of [`TABLE`] runs.
#[test]
fn b20_the_shards_cover_every_table_entry() {
    assert_eq!(B20_SHARDS, (0..SHARDS).collect::<Vec<_>>());
}

// Bodies sent as arrays (`ApiJson` reads every struct from an object only).

/// The most members an object of a suite body may have: every order of its values is one
/// request, so seven would be 5,040 requests for that object alone.
const MAX_MEMBERS: usize = 6;

/// The 2xx body of every table entry and public operation, and a user with identities (an
/// array of structs), each with one object rewritten as an array of its values: the body
/// itself or an object nested in it, in an object or an array. Every order of the values
/// answers 400 `request.malformed`, so the struct's field order, which serde would otherwise
/// accept, does too. A field the document gives as a free-form object (a WebAuthn credential)
/// or as a page body is any JSON to `ApiJson`, so is left as it is. Every operation that takes a body is sent.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn every_body_sent_as_an_array_is_malformed(pool: PgPool) {
    let world = World::new(pool).await;
    let app = router(world.state()).layer(Extension(ConnectInfo(CLIENT)));
    let doc = document();
    let mut sender = ArraySender::new(&app, &doc);
    for entry in TABLE {
        let org = world.org().await;
        let (call, cookie) = allowed_call(&org, &entry.access).await;
        sender
            .send(entry.operation_id, org.id, &call, Some(&cookie))
            .await;
    }
    let public: Vec<&str> = PUBLIC
        .iter()
        .copied()
        .filter(|id| sender.takes_body(id))
        .collect();
    for id in public {
        let org = world.org().await;
        let (body, cookie) = public_request(&world, &org, id).await;
        let call = Call {
            params: Vec::new(),
            body,
            at: At::Org,
        };
        sender.send(id, org.id, &call, cookie.as_deref()).await;
    }
    let org = world.org().await;
    let (mut call, cookie) = allowed_call(&org, &entry_of("createUser").access).await;
    call.body = Some(serde_json::json!({
        "display_name": "Casey Example",
        "identities": [{ "provider_key": "suite", "subject": "1000002" }],
    }));
    sender
        .send("createUser", org.id, &call, Some(&cookie))
        .await;
    let failures = sender.finish();
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Sends the array forms of bodies and keeps what answered otherwise.
struct ArraySender<'a> {
    app: &'a Router,
    doc: &'a Value,
    operations: Vec<Operation>,
    sent: Vec<String>,
    failures: Vec<String>,
}

impl<'a> ArraySender<'a> {
    fn new(app: &'a Router, doc: &'a Value) -> Self {
        Self {
            app,
            doc,
            operations: operations(doc).expect("named operations"),
            sent: Vec::new(),
            failures: Vec::new(),
        }
    }

    fn operation(&self, id: &str) -> Operation {
        self.operations
            .iter()
            .find(|op| op.id == id)
            .unwrap_or_else(|| panic!("{id} is not in the document"))
            .clone()
    }

    /// The JSON schema of the operation's request body; `Null` when it takes none.
    fn body_schema(&self, op: &Operation) -> &'a Value {
        let method = op.method.as_str().to_lowercase();
        &self.doc["paths"][&op.path][method]["requestBody"]["content"]["application/json"]["schema"]
    }

    fn takes_body(&self, id: &str) -> bool {
        !self.body_schema(&self.operation(id)).is_null()
    }

    /// Every array form of `call`'s body, if it has one, as `cookie` in `org`.
    async fn send(&mut self, id: &str, org: Uuid, call: &Call, cookie: Option<&str>) {
        let op = self.operation(id);
        let Some(body) = &call.body else { return };
        for form in array_forms(self.doc, self.body_schema(&op), body) {
            let call = Call {
                params: call.params.clone(),
                body: Some(form),
                at: call.at,
            };
            let answer = call_op(self.app, &op, &call, org, cookie).await;
            let result =
                answer.and_then(|a| a.expect(StatusCode::BAD_REQUEST, "request.malformed"));
            if let Err(why) = result {
                let body = call.body.expect("a body");
                self.failures.push(format!("{id} with {body}: {why}"));
            }
        }
        self.sent.push(id.to_owned());
    }

    /// The failures, and each operation that takes a body but was not sent.
    fn finish(self) -> Vec<String> {
        let unsent = self
            .operations
            .iter()
            .filter(|op| !self.body_schema(op).is_null() && !self.sent.contains(&op.id));
        let unsent: Vec<String> = unsent
            .map(|op| format!("{} takes a body but none was sent", op.id))
            .collect();
        self.failures.into_iter().chain(unsent).collect()
    }
}

/// The table entry of `id`.
fn entry_of(id: &str) -> &'static Entry {
    TABLE
        .iter()
        .find(|entry| entry.operation_id == id)
        .unwrap_or_else(|| panic!("{id} has no table entry"))
}

/// The call of `access`'s 2xx case and the cookie that passes its checks.
async fn allowed_call(org: &Org, access: &Access) -> (Call, String) {
    match access {
        Access::AuthOnly(setup) => (setup(org).await, org.user().await.0),
        Access::Own { setup, .. } => {
            let (cookie, owner) = org.user().await;
            (setup(org, owner).await, cookie)
        }
        Access::Mine(setup) => {
            let (cookie, caller) = org.user().await;
            let mine = setup(org, caller).await;
            let cookie = mine.cookie(Some(&cookie)).expect("a session");
            (mine.call, cookie)
        }
        Access::Permission {
            key, extra, setup, ..
        } => {
            let call = setup(org).await;
            let keys = with_extra(key, extra);
            let cookie = org.holder(&keys, call.at).await;
            (call, cookie)
        }
    }
}

/// `value`, whose schema in `doc` is `schema`, with one of its objects replaced by an array of
/// its values, in every order: `value` itself, or an object below it, in an object or an array,
/// that is not in a free-form field.
fn array_forms(doc: &Value, schema: &Value, value: &Value) -> Vec<Value> {
    let schema = resolve(doc, schema);
    match value {
        Value::Object(fields) => object_forms(doc, schema, fields),
        Value::Array(items) if !free_form(&schema["items"]) => {
            let mut forms = Vec::new();
            for (i, item) in items.iter().enumerate() {
                for form in array_forms(doc, &schema["items"], item) {
                    let mut rewritten = items.clone();
                    rewritten[i] = form;
                    forms.push(Value::Array(rewritten));
                }
            }
            forms
        }
        _ => Vec::new(),
    }
}

/// [`array_forms`] of an object: its values in every order, then each field's forms.
fn object_forms(
    doc: &Value,
    schema: &Value,
    fields: &serde_json::Map<String, Value>,
) -> Vec<Value> {
    let values: Vec<Value> = fields.values().cloned().collect();
    let mut forms: Vec<Value> = orders(&values).into_iter().map(Value::Array).collect();
    for (key, value) in fields {
        let field = &schema["properties"][key];
        if free_form(field) {
            continue;
        }
        for form in array_forms(doc, field, value) {
            let mut rewritten = fields.clone();
            rewritten.insert(key.clone(), form);
            forms.push(Value::Object(rewritten));
        }
    }
    forms
}

/// `schema` with a `$ref`, or the `$ref` of a `oneOf` (an `Option` of a struct), followed.
fn resolve<'a>(doc: &'a Value, schema: &'a Value) -> &'a Value {
    let reference = schema["$ref"].as_str().or_else(|| {
        schema["oneOf"]
            .as_array()
            .and_then(|options| options.iter().find_map(|option| option["$ref"].as_str()))
    });
    match reference.and_then(|r| r.strip_prefix("#/components/schemas/")) {
        Some(name) => &doc["components"]["schemas"][name],
        None => schema,
    }
}

/// An object the document says nothing more about, or a page body: `ApiJson` takes any JSON
/// there. A page body is a `serde_json::Value` that `stepclimb_core::page_doc` parses after
/// `ApiJson`, and answers 422 `page.body_invalid` for anything outside its schema (M1 spec 07
/// B7); `tests/pages.rs` covers its shapes.
fn free_form(schema: &Value) -> bool {
    let page_body = schema["$ref"] == "#/components/schemas/PageDoc";
    page_body
        || (schema["type"] == "object"
            && schema.get("properties").is_none()
            && schema.get("$ref").is_none())
}

/// Every order of `values`, at most [`MAX_MEMBERS`] of them.
fn orders(values: &[Value]) -> Vec<Vec<Value>> {
    assert!(
        values.len() <= MAX_MEMBERS,
        "an object of {} members is {}+ requests, one per order of its values: split the body \
         or send fewer orders (the struct's field order is the one that must be among them)",
        values.len(),
        (1..=values.len()).product::<usize>(),
    );
    if values.is_empty() {
        return vec![Vec::new()];
    }
    let mut all = Vec::new();
    for (i, first) in values.iter().enumerate() {
        let mut rest = values.to_vec();
        rest.remove(i);
        for mut order in orders(&rest) {
            order.insert(0, first.clone());
            all.push(order);
        }
    }
    all
}

#[test]
fn array_forms_cover_every_order_at_every_depth_but_free_form_fields() {
    let doc = serde_json::json!({ "components": { "schemas": {
        "Inner": { "type": "object", "properties": { "c": {}, "d": {} } },
    } } });
    let schema = serde_json::json!({ "type": "object", "properties": {
        "a": { "type": "array", "items": { "$ref": "#/components/schemas/Inner" } },
        "b": { "oneOf": [{ "type": "null" }, { "$ref": "#/components/schemas/Inner" }] },
        "e": { "type": "object" },
    } });
    let body = serde_json::json!({
        "a": [{ "c": 5, "d": 6 }],
        "b": { "c": 2, "d": [3] },
        "e": { "f": 4 },
    });
    let forms = array_forms(&doc, &schema, &body);
    assert_eq!(forms.len(), 6 + 2 + 2, "{forms:#?}");
    let a = serde_json::json!([{ "c": 5, "d": 6 }]);
    let b = serde_json::json!({ "c": 2, "d": [3] });
    let e = serde_json::json!({ "f": 4 });
    assert!(forms.contains(&serde_json::json!([a, b, e])));
    assert!(forms.contains(&serde_json::json!([e, b, a])));
    assert!(forms.contains(&serde_json::json!({ "a": [[5, 6]], "b": b, "e": e })));
    assert!(forms.contains(&serde_json::json!({ "a": [[6, 5]], "b": b, "e": e })));
    assert!(forms.contains(&serde_json::json!({ "a": a, "b": [2, [3]], "e": e })));
    assert!(forms.contains(&serde_json::json!({ "a": a, "b": [[3], 2], "e": e })));
    assert!(!forms.contains(&serde_json::json!({ "a": a, "b": b, "e": [4] })));
}

#[test]
#[should_panic(expected = "an object of 7 members is 5040+ requests")]
fn array_forms_refuse_an_object_with_too_many_members() {
    let body: serde_json::Map<String, Value> =
        (0..7).map(|i| (format!("m{i}"), Value::from(i))).collect();
    array_forms(&Value::Null, &Value::Null, &Value::Object(body));
}

// The runner.

/// What the runner did with one entry.
#[derive(Debug, Default)]
struct Report {
    /// The cases that ran, in order.
    ran: Vec<&'static str>,
    failures: Vec<String>,
}

impl Report {
    fn record(&mut self, op: &Operation, case: &'static str, result: Result<(), String>) {
        self.ran.push(case);
        if let Err(why) = result {
            self.failures.push(format!(
                "{} ({} {}) {case}: {why}",
                op.id, op.method, op.path
            ));
        }
    }
}

async fn run(world: &World, app: &Router, op: &Operation, access: &Access) -> Report {
    let mut report = Report::default();
    match access {
        Access::AuthOnly(setup) => {
            report.record(op, "401", anonymous(world, app, op, *setup).await);
            report.record(op, "2xx", allowed(world, app, op, &[], *setup).await);
            let result = every_feature_off(world, app, op, &[], *setup).await;
            report.record(op, "every feature off", result);
            let result = cross_organization(world, app, op, None, *setup).await;
            report.record(op, "cross-organization", result);
        }
        Access::Own { not_found, setup } => {
            run_own(world, app, op, not_found, *setup, &mut report).await
        }
        Access::Mine(setup) => run_mine(world, app, op, *setup, &mut report).await,
        Access::Permission {
            key,
            target,
            extra,
            feature,
            feature_by_target,
            denied_also,
            setup,
        } => {
            let Some(info) = perm::ALL.iter().find(|p| p.key() == *key) else {
                report.record(op, "catalog", Err(format!("{key} is not in perm::ALL")));
                return report;
            };
            if let Some(feature) = feature
                && !feature::ALL.contains(feature)
            {
                report.record(
                    op,
                    "catalog",
                    Err(format!("{feature} is not in feature::ALL")),
                );
                return report;
            }
            report.record(op, "401", anonymous(world, app, op, *setup).await);
            if let Some(feature) = feature {
                let off = (StatusCode::FORBIDDEN, "entitlement.feature_unavailable");
                let result = feature_off(world, app, op, feature, *setup, false, off).await;
                report.record(op, "feature off", result);
                if op.path.contains('{') {
                    let expected = match target {
                        Target::Scoped { not_found, .. } if *feature_by_target => {
                            (StatusCode::NOT_FOUND, *not_found)
                        }
                        _ => off,
                    };
                    let result = feature_off(world, app, op, feature, *setup, true, expected).await;
                    report.record(op, "feature off, unknown ids", result);
                }
            }
            let without = with_extra(key, denied_also);
            report.record(op, "403", denied(world, app, op, &without, *setup).await);
            if let Target::Scoped { not_found, .. } = target
                && info.grantable().contains(&ScopeType::Account)
                && let Some(result) = hidden(world, app, op, not_found, *setup).await
            {
                report.record(op, "404", result);
            }
            let keys = with_extra(key, extra);
            report.record(op, "2xx", allowed(world, app, op, &keys, *setup).await);
            if feature.is_none() {
                let result = every_feature_off(world, app, op, &keys, *setup).await;
                report.record(op, "every feature off", result);
            }
            let not_found = match target {
                Target::Scoped { cross_org, .. } => Some(*cross_org),
                Target::Org | Target::AnyScope => None,
            };
            let result = cross_organization(world, app, op, not_found, *setup).await;
            report.record(op, "cross-organization", result);
        }
    }
    report
}

async fn anonymous(
    world: &World,
    app: &Router,
    op: &Operation,
    setup: Setup,
) -> Result<(), String> {
    let org = world.org().await;
    let call = setup(&org).await;
    let answer = call_op(app, op, &call, org.id, None).await?;
    answer.expect(StatusCode::UNAUTHORIZED, "auth.unauthenticated")
}

/// A holder of every catalog permission but `without` (the route's, and any other it is
/// granted on) gets 403 `permission.denied`.
async fn denied(
    world: &World,
    app: &Router,
    op: &Operation,
    without: &[&str],
    setup: Setup,
) -> Result<(), String> {
    let org = world.org().await;
    let call = setup(&org).await;
    let others: Vec<&str> = all_keys()
        .into_iter()
        .filter(|k| !without.contains(k))
        .collect();
    let cookie = org.holder(&others, call.at).await;
    let answer = call_op(app, op, &call, org.id, Some(&cookie)).await?;
    answer.expect(StatusCode::FORBIDDEN, "permission.denied")
}

/// `None` when the target's scope is the organization, which has no sibling.
async fn hidden(
    world: &World,
    app: &Router,
    op: &Operation,
    not_found: &str,
    setup: Setup,
) -> Option<Result<(), String>> {
    let org = world.org().await;
    let call = setup(&org).await;
    let At::Account(account) = call.at else {
        return None;
    };
    let sibling = org.sibling(account).await;
    let cookie = org.holder(&all_keys(), At::Account(sibling)).await;
    Some(match call_op(app, op, &call, org.id, Some(&cookie)).await {
        Ok(answer) => answer.expect(StatusCode::NOT_FOUND, not_found),
        Err(why) => Err(why),
    })
}

/// The organization's plan has every catalog feature but `feature`: a user holding every
/// permission at the target's scope gets `expected` (403 `entitlement.feature_unavailable`) and
/// the organization's rows stay as they were. With `unknown_ids`, every path parameter names an
/// id that does not exist, which must not change the answer (spec 10 B9), unless the target
/// decides the feature ([`gated_by_target`]): then `expected` is the target's 404.
async fn feature_off(
    world: &World,
    app: &Router,
    op: &Operation,
    feature: &str,
    setup: Setup,
    unknown_ids: bool,
    expected: (StatusCode, &str),
) -> Result<(), String> {
    let org = world.org().await;
    let mut call = setup(&org).await;
    if unknown_ids {
        call.params = call
            .params
            .iter()
            .map(|(name, _)| (*name, new_id()))
            .collect();
    }
    let cookie = org.holder(&all_keys(), call.at).await;
    org.plan_without(feature).await;
    let before = snapshot(&world.pool, org.id).await?;
    let answer = call_op(app, op, &call, org.id, Some(&cookie)).await?;
    let mut wrong = Vec::new();
    wrong.extend(answer.expect(expected.0, expected.1).err());
    if snapshot(&world.pool, org.id).await? != before {
        wrong.push("the organization's rows changed".to_owned());
    }
    if wrong.is_empty() {
        Ok(())
    } else {
        Err(wrong.join("; "))
    }
}

/// With `keys` empty, a user with no role at all.
async fn allowed(
    world: &World,
    app: &Router,
    op: &Operation,
    keys: &[&str],
    setup: Setup,
) -> Result<(), String> {
    let org = world.org().await;
    let call = setup(&org).await;
    let cookie = if keys.is_empty() {
        org.user().await.0
    } else {
        org.holder(keys, call.at).await
    };
    let answer = call_op(app, op, &call, org.id, Some(&cookie)).await?;
    answer.expect_success()
}

/// [`allowed`]'s request with the organization on a plan with no catalog feature: anything but
/// 403 `entitlement.feature_unavailable`, since the entry declares no feature.
async fn every_feature_off(
    world: &World,
    app: &Router,
    op: &Operation,
    keys: &[&str],
    setup: Setup,
) -> Result<(), String> {
    let org = world.org().await;
    let call = setup(&org).await;
    let cookie = if keys.is_empty() {
        org.user().await.0
    } else {
        org.holder(keys, call.at).await
    };
    org.plan_with(|_| false).await;
    let answer = call_op(app, op, &call, org.id, Some(&cookie)).await?;
    if answer.code().as_deref() == Some("entitlement.feature_unavailable") {
        return Err(format!(
            "its entry names no feature, but it answered {} {}",
            answer.status, answer.body
        ));
    }
    Ok(())
}

/// An Organization admin of org A calls with the target built in org B, which also holds a
/// user, a session, a custom role and an assignment: `not_found` (or 2xx when the route has no
/// target), org B's rows unchanged, and none of their identifying values answered. Every one of
/// the three that fails is reported.
async fn cross_organization(
    world: &World,
    app: &Router,
    op: &Operation,
    not_found: Option<&str>,
    setup: Setup,
) -> Result<(), String> {
    let a = world.org().await;
    let admin = a.holder_of_builtin("org_admin", At::Org).await;
    let b = world.org().await;
    b.seed().await;
    let call = setup(&b).await;
    called_from_org_a(&world.pool, app, op, not_found, (a.id, &admin), &b, &call).await
}

/// `call`, built in org B, made by `admin` of org A (`a`): `not_found` (or 2xx), org B's rows
/// unchanged, and none of their identifying values answered.
async fn called_from_org_a(
    pool: &PgPool,
    app: &Router,
    op: &Operation,
    not_found: Option<&str>,
    (a, admin): (Uuid, &str),
    b: &Org,
    call: &Call,
) -> Result<(), String> {
    let before = snapshot(pool, b.id).await?;
    let answer = call_op(app, op, call, a, Some(admin)).await?;
    let mut wrong = Vec::new();
    let status = match not_found {
        Some(code) => answer.expect(StatusCode::NOT_FOUND, code),
        None => answer.expect_success(),
    };
    wrong.extend(status.err());
    if snapshot(pool, b.id).await? != before {
        wrong.push("org B's rows changed".to_owned());
    }
    let mut identifying = Vec::new();
    identifiers(&before, &b.slug, &mut identifying);
    if let Some(value) = identifying
        .iter()
        .find(|v| answer.body.contains(v.as_str()))
    {
        wrong.push(format!("the response holds org B's value {value:?}"));
    }
    if wrong.is_empty() {
        Ok(())
    } else {
        Err(wrong.join("; "))
    }
}

/// The cases of an [`Access::Own`] entry.
async fn run_own(
    world: &World,
    app: &Router,
    op: &Operation,
    not_found: &'static str,
    setup: OwnSetup,
    report: &mut Report,
) {
    let anonymous = async {
        let org = world.org().await;
        let (_, owner) = org.user().await;
        let call = setup(&org, owner).await;
        call_op(app, op, &call, org.id, None)
            .await?
            .expect(StatusCode::UNAUTHORIZED, "auth.unauthenticated")
    };
    report.record(op, "401", anonymous.await);
    let owner = async {
        let org = world.org().await;
        let (cookie, owner) = org.user().await;
        let call = setup(&org, owner).await;
        call_op(app, op, &call, org.id, Some(&cookie))
            .await?
            .expect_success()
    };
    report.record(op, "2xx", owner.await);
    // As `every_feature_off`, for the owner: the entry declares no feature.
    let featureless = async {
        let org = world.org().await;
        let (cookie, owner) = org.user().await;
        let call = setup(&org, owner).await;
        org.plan_with(|_| false).await;
        let answer = call_op(app, op, &call, org.id, Some(&cookie)).await?;
        if answer.code().as_deref() == Some("entitlement.feature_unavailable") {
            return Err(format!(
                "its entry names no feature, but it answered {} {}",
                answer.status, answer.body
            ));
        }
        Ok(())
    };
    report.record(op, "every feature off", featureless.await);
    report.record(
        op,
        "other-user",
        other_users(world, app, op, not_found, setup).await,
    );
    let cross = async {
        let a = world.org().await;
        let admin = a.holder_of_builtin("org_admin", At::Org).await;
        let b = world.org().await;
        b.seed().await;
        let (_, owner) = b.user().await;
        let call = setup(&b, owner).await;
        let admin = (a.id, admin.as_str());
        called_from_org_a(&world.pool, app, op, Some(not_found), admin, &b, &call).await
    };
    report.record(op, "cross-organization", cross.await);
}

/// The cases of an [`Access::Mine`] entry.
async fn run_mine(
    world: &World,
    app: &Router,
    op: &Operation,
    setup: MineSetup,
    report: &mut Report,
) {
    let anonymous = async {
        let org = world.org().await;
        let (_, caller) = org.user().await;
        let mine = setup(&org, caller).await;
        call_op(app, op, &mine.call, org.id, mine.cookie(None).as_deref())
            .await?
            .expect(StatusCode::UNAUTHORIZED, "auth.unauthenticated")
    };
    report.record(op, "401", anonymous.await);
    let caller = async {
        let org = world.org().await;
        let (cookie, caller) = org.user().await;
        let mine = setup(&org, caller).await;
        call_op(
            app,
            op,
            &mine.call,
            org.id,
            mine.cookie(Some(&cookie)).as_deref(),
        )
        .await?
        .expect_success()
    };
    report.record(op, "2xx", caller.await);
    let featureless = async {
        let org = world.org().await;
        let (cookie, caller) = org.user().await;
        let mine = setup(&org, caller).await;
        org.plan_with(|_| false).await;
        let cookie = mine.cookie(Some(&cookie));
        let answer = call_op(app, op, &mine.call, org.id, cookie.as_deref()).await?;
        if answer.code().as_deref() == Some("entitlement.feature_unavailable") {
            return Err(format!(
                "its entry names no feature, but it answered {} {}",
                answer.status, answer.body
            ));
        }
        Ok(())
    };
    report.record(op, "every feature off", featureless.await);
    let cross = async {
        let a = world.org().await;
        let (admin_cookie, admin) = a.admin().await;
        let b = world.org().await;
        b.seed().await;
        let mine = setup(&a, admin).await;
        let cookie = mine.cookie(Some(&admin_cookie)).expect("a session");
        let admin = (a.id, cookie.as_str());
        called_from_org_a(&world.pool, app, op, None, admin, &b, &mine.call).await
    };
    report.record(op, "cross-organization", cross.await);
}

/// Another user of the same organization, an Organization admin, calls with the owner's row:
/// `not_found`, and the organization's rows unchanged.
async fn other_users(
    world: &World,
    app: &Router,
    op: &Operation,
    not_found: &str,
    setup: OwnSetup,
) -> Result<(), String> {
    let org = world.org().await;
    let (_, owner) = org.user().await;
    let call = setup(&org, owner).await;
    let admin = org.holder_of_builtin("org_admin", At::Org).await;
    let before = snapshot(&world.pool, org.id).await?;
    let answer = call_op(app, op, &call, org.id, Some(&admin)).await?;
    answer.expect(StatusCode::NOT_FOUND, not_found)?;
    if snapshot(&world.pool, org.id).await? != before {
        return Err("the owner's rows changed".to_owned());
    }
    Ok(())
}

/// Tables whose rows belong to an organization without an `org_id` column, with the condition
/// that picks organization `$1`'s rows of `t`.
const SCOPED_OTHERWISE: &[(&str, &str)] = &[
    ("organizations", "t.id = $1"),
    (
        "sessions",
        "t.user_id IN (SELECT id FROM users WHERE org_id = $1)",
    ),
];

/// Tables that hold no organization's rows. `plans` holds instance-wide plan definitions (spec
/// 10 Data); an organization's plan is its `org_plans` row, which has `org_id`. `job_schedules`
/// holds one row per periodic job kind (spec m1/01 Data).
const INSTANCE_ONLY: &[&str] = &["_sqlx_migrations", "job_schedules", "plans"];

/// Every row of organization `org`, by table: each table with `org_id`, and each table of
/// [`SCOPED_OTHERWISE`]. A table of the schema in neither, nor in [`INSTANCE_ONLY`], is an error,
/// so a new table scoped through another one cannot be left out of the comparison.
async fn snapshot(pool: &PgPool, org: Uuid) -> Result<Value, String> {
    let tables: Vec<(String, bool)> = sqlx::query_as(
        "SELECT t.table_name::text, EXISTS (
                SELECT 1 FROM information_schema.columns c
                WHERE c.table_schema = t.table_schema AND c.table_name = t.table_name
                  AND c.column_name = 'org_id')
         FROM information_schema.tables t
         WHERE t.table_schema = 'public' AND t.table_type = 'BASE TABLE'
         ORDER BY 1",
    )
    .fetch_all(pool)
    .await
    .expect("list tables");
    let mut rows = serde_json::Map::new();
    let mut unscoped = Vec::new();
    for (table, has_org_id) in tables {
        let other = SCOPED_OTHERWISE.iter().find(|(name, _)| *name == table);
        let condition = match (has_org_id, other) {
            (_, Some((_, condition))) => *condition,
            (true, None) => "t.org_id = $1",
            (false, None) if INSTANCE_ONLY.contains(&table.as_str()) => continue,
            (false, None) => {
                unscoped.push(table);
                continue;
            }
        };
        let sql = format!(
            "SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY to_jsonb(t)::text), '[]')::text
             FROM \"{table}\" t WHERE {condition}"
        );
        let json: String = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .bind(org)
            .fetch_one(pool)
            .await
            .expect("read rows");
        rows.insert(table, serde_json::from_str(&json).expect("JSON rows"));
    }
    if unscoped.is_empty() {
        Ok(Value::Object(rows))
    } else {
        Err(format!(
            "tables with no org_id and no entry in SCOPED_OTHERWISE or INSTANCE_ONLY in \
             tests/permissions.rs: {unscoped:?}"
        ))
    }
}

/// Every string in `value` that is a UUID or contains the organization's `slug` (its name, and
/// the seeded names, emails and role key).
fn identifiers(value: &Value, slug: &str, out: &mut Vec<String>) {
    match value {
        Value::String(s) if Uuid::try_parse(s).is_ok() || s.contains(slug) => out.push(s.clone()),
        Value::Array(items) => items.iter().for_each(|v| identifiers(v, slug, out)),
        Value::Object(map) => map.values().for_each(|v| identifiers(v, slug, out)),
        _ => {}
    }
}

/// `key` and the `extra` keys other than it.
fn with_extra(key: &'static str, extra: &[&'static str]) -> Vec<&'static str> {
    let others = extra.iter().copied().filter(|k| *k != key);
    std::iter::once(key).chain(others).collect()
}

fn all_keys() -> Vec<&'static str> {
    perm::ALL.iter().map(|p| p.key()).collect()
}

// Requests.

#[derive(Debug)]
struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
    /// The answer to a `HEAD` request, which has no body: [`Answer::expect`] checks its status
    /// only, the code being in no header.
    head: bool,
}

impl Answer {
    fn code(&self) -> Option<String> {
        serde_json::from_str::<Problem>(&self.body)
            .ok()
            .map(|p| p.code)
    }

    fn expect(&self, status: StatusCode, code: &str) -> Result<(), String> {
        let code_matches = self.head || self.code().as_deref() == Some(code);
        if self.status == status && code_matches {
            Ok(())
        } else {
            Err(format!(
                "expected {status} {code}, got {} {}",
                self.status, self.body
            ))
        }
    }

    fn expect_success(&self) -> Result<(), String> {
        if self.status.is_success() {
            Ok(())
        } else {
            Err(format!("expected 2xx, got {} {}", self.status, self.body))
        }
    }
}

/// `call` against `op`, in the organization `org`.
async fn call_op(
    app: &Router,
    op: &Operation,
    call: &Call,
    org: Uuid,
    cookie: Option<&str>,
) -> Result<Answer, String> {
    let mut uri = op.path.clone();
    for (name, value) in &call.params {
        uri = uri.replace(&format!("{{{name}}}"), &value.to_string());
    }
    if uri.contains('{') {
        return Err(format!("the setup leaves a path parameter unset: {uri}"));
    }
    if let Some((_, query)) = QUERIES.iter().find(|(id, _)| *id == op.id) {
        uri = format!("{uri}?{query}");
    }
    let headers = HEADERS
        .iter()
        .find(|(id, _)| *id == op.id)
        .map_or(&[][..], |(_, headers)| *headers);
    let request = Sent {
        method: &op.method,
        uri: &uri,
        headers,
    };
    Ok(send_with(app, request, org, cookie, call.body.as_ref()).await)
}

/// One request, same-origin as the browser app sends it, in the organization `org`.
async fn send(
    app: &Router,
    method: &Method,
    uri: &str,
    org: Uuid,
    cookie: Option<&str>,
    body: Option<&Value>,
) -> Answer {
    let request = Sent {
        method,
        uri,
        headers: &[],
    };
    send_with(app, request, org, cookie, body).await
}

/// The method, URI and extra headers of a request [`send_with`] sends.
struct Sent<'a> {
    method: &'a Method,
    uri: &'a str,
    headers: &'a [(&'a str, &'a str)],
}

/// [`send`] with `sent`'s extra headers; one of them that is `Content-Type` replaces the JSON one.
async fn send_with(
    app: &Router,
    sent: Sent<'_>,
    org: Uuid,
    cookie: Option<&str>,
    body: Option<&Value>,
) -> Answer {
    let mut request = Request::builder()
        .method(sent.method)
        .uri(sent.uri)
        .header(TEST_ORG_HEADER, org.to_string())
        .header("sec-fetch-site", "same-origin");
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    for (name, value) in sent.headers {
        request = request.header(*name, *value);
    }
    let typed = sent.headers.iter().any(|(name, _)| *name == "content-type");
    let body = match body {
        Some(json) => {
            let json = json.to_string();
            if !typed {
                request = request.header(header::CONTENT_TYPE, "application/json");
            }
            request = request.header(header::CONTENT_LENGTH, json.len());
            Body::from(json)
        }
        None => Body::empty(),
    };
    let response = app
        .clone()
        .oneshot(request.body(body).expect("a valid request"))
        .await
        .expect("the router never fails");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("the body is readable")
        .to_bytes();
    Answer {
        status,
        headers,
        body: String::from_utf8_lossy(&bytes).into_owned(),
        head: *sent.method == Method::HEAD,
    }
}

// Fixtures.

/// 2025-11-13T02:13:20Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the sign-in specs and the admin routes that build these rows"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// The application state every case's router and setup share: one key ring, so a flow a setup
/// starts opens in the router.
fn state_for(pool: &PgPool) -> AppState {
    AppState {
        db: pool.clone(),
        clock: ManualClock::new(now()).clock(),
        keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
            .expect("a valid test key"),
        public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
        allowed_origins: Vec::new(),
        rate_limits: Default::default(),
        // The insecure switch on, as the test setup sets it (spec 05 Q4): the mock IdP of the
        // provider sign-in case listens on http loopback.
        passwords: Default::default(),
        storage: None,
        providers: stepclimb_server::providers::Providers::new(true).expect("provider clients"),
    }
}

/// The test's database, with the built-in roles synced, and a local directory the router stores
/// course files in, removed on drop.
struct World {
    pool: PgPool,
    files: PathBuf,
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.files);
    }
}

impl World {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
        let files = std::env::temp_dir().join(format!("stepclimb-suite-{}", new_id()));
        std::fs::create_dir_all(&files).expect("create the storage directory");
        Self { pool, files }
    }

    fn state(&self) -> AppState {
        let config = StorageConfig::Local {
            root: self.files.clone(),
        };
        let storage = Storage::open(&config, 1024 * 1024).expect("open the directory");
        AppState {
            storage,
            ..state_for(&self.pool)
        }
    }

    /// A new organization with Tower under the root and Ground under Tower.
    async fn org(&self) -> Org {
        let hex = new_id().simple().to_string();
        let slug = format!("org-{}", &hex[hex.len() - 16..]);
        let id = organizations::create(&self.pool, &slug, &slug, now())
            .await
            .expect("create organization")
            .id;
        let root = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .expect("the root");
        let mut org = Org {
            pool: self.pool.clone(),
            id,
            slug,
            tower: root,
            ground: root,
        };
        org.tower = org.account(root, "Tower").await;
        org.ground = org.account(org.tower, "Ground").await;
        org
    }
}

/// One organization of the test's database and its accounts.
struct Org {
    pool: PgPool,
    id: Uuid,
    /// Also its name.
    slug: String,
    tower: Uuid,
    ground: Uuid,
}

impl Org {
    /// An admission webhook with a stand-in sealed secret: the routes never open one.
    async fn admission_webhook(&self) {
        let url = format!("https://{}.example.test/hook", self.slug);
        let change = Change {
            url: Some(&url),
            enabled: None,
        };
        let fresh = Fresh {
            id: new_id(),
            sealed_secret: b"stand-in",
        };
        admission_webhooks::save(
            &self.pool,
            scope(self.id),
            change,
            &fresh,
            now(),
            Actor::Instance,
        )
        .await
        .expect("create the admission webhook");
    }

    /// A new course in `account`, its code naming the slug, published when `published`: its id.
    async fn course(&self, account: Uuid, published: bool) -> Uuid {
        let hex = new_id().simple().to_string();
        let new = courses::NewCourse {
            account_id: account,
            title: format!("Course of {}", self.slug),
            code: format!("C{}", &hex[hex.len() - 12..]),
        };
        let id = courses::create(&self.pool, scope(self.id), &new, now(), Actor::Instance)
            .await
            .expect("create course")
            .id;
        if published {
            let status = courses::Status::Published;
            courses::set_status(
                &self.pool,
                scope(self.id),
                id,
                status,
                now(),
                Actor::Instance,
            )
            .await
            .expect("publish course");
        }
        id
    }

    /// An upload of 15 bytes started in `course` by a new user: the upload's and its video's
    /// ids. 15 bytes is the JSON text of [`chunk_of_video_upload_in_tower`]'s body, so that chunk
    /// completes the upload and the 1 MiB minimum of a chunk that does not (D15) is not met.
    async fn video_upload(&self, course: Uuid) -> (Uuid, Uuid) {
        let (_, user) = self.user().await;
        let title = format!("Video of {}", self.slug);
        let new = videos::NewUpload {
            course_id: course,
            title: &title,
            length: 15,
            created_by: user,
        };
        let mut tx = self.pool.begin().await.expect("begin");
        let (video, upload) = videos::create_upload(&mut tx, scope(self.id), &new, now())
            .await
            .expect("start an upload");
        tx.commit().await.expect("commit");
        (upload.id, video.id)
    }

    /// A course file in `folder` of `course`, its bytes naming the slug and a new id: its id.
    async fn course_file(&self, course: Uuid, folder: &str) -> Uuid {
        let mut hasher = files::Sha256Hasher::new();
        hasher.update(format!("File of {} {}", self.slug, new_id()).as_bytes());
        let new = files::NewFile {
            hashed: hasher.finish(),
            mime: "text/plain; charset=utf-8".into(),
        };
        let file = files::insert(&self.pool, scope(self.id), &new, now())
            .await
            .expect("insert file")
            .id;
        let place = (
            &course_files::Folder::parse(folder).expect("a folder"),
            &course_files::Name::parse(&format!("{}.txt", new_id())).expect("a name"),
        );
        let mut conn = self.pool.acquire().await.expect("a connection");
        course_files::create(&mut conn, scope(self.id), course, file, place, now())
            .await
            .expect("create course file")
            .id
    }

    /// A new page of `course`, by a new user: its id and its version 1's.
    async fn page(&self, course: Uuid) -> (Uuid, Uuid) {
        let (_, author) = self.user().await;
        let new = pages::NewPage {
            title: format!("Page of {}", self.slug),
            slug: Some(pages::Slug::parse(&format!("p-{}", new_id().simple())).expect("a slug")),
            body: PageDoc::empty(),
        };
        let mut conn = self.pool.acquire().await.expect("a connection");
        let page = pages::create(&mut conn, scope(self.id), course, &new, author, now())
            .await
            .expect("create page");
        (page.id, page.current.id)
    }

    /// A new version of `page` saved over `base`: its id.
    async fn save_page(&self, page: Uuid, base: Uuid) -> Uuid {
        let (_, author) = self.user().await;
        let body = PageDoc::parse_new(&serde_json::json!({ "type": "doc", "content": [
            { "type": "paragraph", "content": [{ "type": "text", "text": "Second" }] },
        ] }))
        .expect("a body");
        let save = pages::Save {
            body: &body,
            base_version_id: base,
            author_id: author,
        };
        let mut conn = self.pool.acquire().await.expect("a connection");
        pages::save(&mut conn, scope(self.id), page, save, now())
            .await
            .expect("save page")
            .current
            .id
    }

    /// A new module, last in `course`: its id.
    async fn module(&self, course: Uuid) -> Uuid {
        let title = format!("Module of {}", self.slug);
        modules::create(&self.pool, scope(self.id), course, &title, now())
            .await
            .expect("create module")
            .id
    }

    /// A new header, last in `module`: its id.
    async fn header(&self, module: Uuid) -> Uuid {
        let new = items::NewItem {
            kind: items::Kind::Header,
            title: format!("Header of {}", self.slug),
            ref_id: None,
            url: None,
            position: None,
        };
        items::create(&self.pool, scope(self.id), module, &new, now())
            .await
            .expect("create item")
            .id
    }

    /// Publish `module` and its item `item`.
    async fn publish(&self, module: Uuid, item: Uuid) {
        let patch = modules::ModulePatch {
            published: Some(true),
            ..Default::default()
        };
        modules::update(&self.pool, scope(self.id), module, &patch, now())
            .await
            .expect("publish module");
        let patch = items::ItemPatch {
            published: Some(true),
            ..Default::default()
        };
        items::update(&self.pool, scope(self.id), item, &patch, now())
            .await
            .expect("publish item");
    }

    /// A new `file` item naming the course file `course_file`, last in `module`: its id.
    async fn file_item(&self, module: Uuid, course_file: Uuid) -> Uuid {
        let new = items::NewItem {
            kind: items::Kind::File,
            title: format!("Handout of {}", self.slug),
            ref_id: Some(course_file),
            url: None,
            position: None,
        };
        items::create(&self.pool, scope(self.id), module, &new, now())
            .await
            .expect("create item")
            .id
    }

    /// A new external link, last in `module`: its id.
    async fn link(&self, module: Uuid) -> Uuid {
        let new = items::NewItem {
            kind: items::Kind::ExternalLink,
            title: format!("Link of {}", self.slug),
            ref_id: None,
            url: Some("https://example.com/suite".into()),
            position: None,
        };
        items::create(&self.pool, scope(self.id), module, &new, now())
            .await
            .expect("create item")
            .id
    }

    async fn account(&self, parent: Uuid, name: &str) -> Uuid {
        accounts::create(
            &self.pool,
            scope(self.id),
            parent,
            name,
            now(),
            Actor::Instance,
        )
        .await
        .expect("create account")
        .id
    }

    /// A new account beside `account`, under its parent; under the root itself when `account`
    /// is the root, which still does not contain it.
    async fn sibling(&self, account: Uuid) -> Uuid {
        let parent: Option<Uuid> =
            sqlx::query_scalar("SELECT parent_id FROM accounts WHERE id = $1")
                .bind(account)
                .fetch_one(&self.pool)
                .await
                .expect("the account");
        self.account(parent.unwrap_or(account), "Sibling").await
    }

    /// A user, signed in, with a custom role at the organization, every value naming the slug:
    /// the rows of org B a cross-organization call must neither change nor answer.
    async fn seed(&self) {
        let slug = &self.slug;
        let (_, user) = self
            .named_user(
                format!("Robin {slug}"),
                Some(format!("robin@{slug}.example")),
            )
            .await;
        let role = NewRole {
            key: format!("r_{}", slug.replace('-', "_")),
            name: format!("Role of {slug}"),
            permissions: vec![RoleView::KEY.to_owned()],
        };
        let role = roles::create_custom(&self.pool, scope(self.id), &role, now(), Actor::Instance)
            .await
            .expect("create role")
            .id;
        self.assign(user, role, At::Org).await;
        self.service_account(user, &format!("Key of {slug}")).await;
        let course = self.course(self.tower, true).await;
        self.enrollment(course).await;
    }

    /// A service account of `owner` named `name`, with a new key: its id.
    async fn service_account(&self, owner: Uuid, name: &str) -> Uuid {
        let key = stepclimb_auth::token::ApiKey::generate().expect("a key");
        let new = service_accounts::NewServiceAccount {
            name: name.to_owned(),
            owner_user_id: owner,
            scopes: vec![RoleView::KEY.to_owned()],
        };
        service_accounts::create(
            &self.pool,
            scope(self.id),
            &new,
            key.hash().as_bytes(),
            now(),
            Actor::Instance,
        )
        .await
        .expect("create service account")
        .id
    }

    /// A new user, signed in, with no role: their `Cookie` header value and id.
    async fn user(&self) -> (String, Uuid) {
        self.named_user("Pat Example".into(), None).await
    }

    async fn named_user(&self, display_name: String, email: Option<String>) -> (String, Uuid) {
        let new_user = NewUser {
            display_name,
            given_name: String::new(),
            family_name: String::new(),
            email,
            locale: None,
            tz: "UTC".into(),
            source: Source::Local,
            external_id: None,
        };
        let id = users::create(&self.pool, scope(self.id), &new_user, now())
            .await
            .expect("create user")
            .id;
        let started = session::start(&self.pool, scope(self.id), id, &HeaderMap::new(), now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        (cookie.split(';').next().expect("a pair").to_owned(), id)
    }

    /// `role` held by `user` at `at`: a role assignment's id, or an enrollment's at a course.
    async fn assign(&self, user: Uuid, role: Uuid, at: At) -> Uuid {
        let scope_of = match at {
            At::Org => Scope::Organization,
            At::Account(id) => Scope::Account(id),
            At::Course(course) => return self.enroll(user, role, course).await,
            At::Observing { course, learner } => {
                let enrollment = self.enroll(user, role, course).await;
                sqlx::query(
                    "INSERT INTO enrollment_observees (org_id, enrollment_id, course_id, user_id)
                     VALUES ($1, $2, $3, $4)",
                )
                .bind(self.id)
                .bind(enrollment)
                .bind(course)
                .bind(learner)
                .execute(&self.pool)
                .await
                .expect("observe the learner");
                return enrollment;
            }
        };
        let assignment = NewAssignment {
            user_id: user,
            role_id: role,
            scope: scope_of,
            source: Source::Local,
            external_id: None,
        };
        role_assignments::create(
            &self.pool,
            scope(self.id),
            &assignment,
            now(),
            Actor::Instance,
        )
        .await
        .expect("create assignment")
        .id
    }

    /// A signed-in user holding a new custom role with exactly `permissions` at `at`.
    async fn holder(&self, permissions: &[&str], at: At) -> String {
        let (cookie, user) = self.user().await;
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Suite role".into(),
            permissions: permissions.iter().map(|&p| p.to_owned()).collect(),
        };
        let role = roles::create_custom(&self.pool, scope(self.id), &role, now(), Actor::Instance)
            .await
            .expect("create role")
            .id;
        self.assign(user, role, at).await;
        cookie
    }

    /// Point the organization at a new plan with every catalog feature but `feature` and no
    /// limit, by SQL, since M0 has no API that changes a plan.
    async fn plan_without(&self, feature: &str) {
        self.plan_with(|key| key != feature).await;
    }

    /// Point the organization at a new plan holding the catalog features `includes` accepts
    /// and no limit.
    async fn plan_with(&self, includes: impl Fn(&str) -> bool) {
        let features: serde_json::Map<String, Value> = feature::ALL
            .iter()
            .map(|key| ((*key).to_owned(), Value::Bool(includes(key))))
            .collect();
        let limits: serde_json::Map<String, Value> = Limit::ALL
            .iter()
            .map(|limit| (limit.key().to_owned(), Value::Null))
            .collect();
        let plan = new_id();
        sqlx::query("INSERT INTO plans (id, key, features, limits) VALUES ($1, $2, $3, $4)")
            .bind(plan)
            .bind(format!("test_{}", plan.simple()))
            .bind(Value::Object(features))
            .bind(Value::Object(limits))
            .execute(&self.pool)
            .await
            .expect("insert plan");
        sqlx::query("UPDATE org_plans SET plan_id = $2, source = 'grant' WHERE org_id = $1")
            .bind(self.id)
            .bind(plan)
            .execute(&self.pool)
            .await
            .expect("move to the plan");
    }

    /// A new identity provider of the organization: its id and key. The write functions
    /// belong to specs 05 and 06, so the row is inserted directly.
    async fn provider(&self) -> (Uuid, String) {
        let id = new_id();
        let hex = id.simple().to_string();
        let key = format!("p{}", &hex[hex.len() - 16..]);
        sqlx::query(
            "INSERT INTO identity_providers (id, org_id, key, kind, config)
             VALUES ($1, $2, $3, 'oidc', $4::jsonb)",
        )
        .bind(id)
        .bind(self.id)
        .bind(&key)
        .bind(format!(r#"{{"label": "Provider of {}"}}"#, self.slug))
        .execute(&self.pool)
        .await
        .expect("insert provider");
        (id, key)
    }

    /// Two identities of `user` at a new provider; the second's id.
    async fn two_identities(&self, user: Uuid) -> Uuid {
        let (provider, _) = self.provider().await;
        let mut last = None;
        for subject in ["1000001", "1000002"] {
            let identity = NewIdentity {
                user_id: user,
                provider_id: provider,
                subject: subject.into(),
                attributes: serde_json::Map::new(),
            };
            let linked = identities::link(
                &self.pool,
                scope(self.id),
                &identity,
                now(),
                Actor::Instance,
            )
            .await
            .expect("link identity");
            last = Some(linked.id);
        }
        last.expect("two identities")
    }

    /// `user` enrolled in `course` with `role`, active: the enrollment's id. Inserted by SQL,
    /// since the repository asks a role holding `progress.view_observed` for the learners it
    /// observes (M1 spec 05 B17), and the suite's "every permission" holders name none.
    async fn enroll(&self, user: Uuid, role: Uuid, course: Uuid) -> Uuid {
        let id = new_id();
        sqlx::query(
            "INSERT INTO enrollments (id, org_id, course_id, user_id, role_id)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id)
        .bind(self.id)
        .bind(course)
        .bind(user)
        .bind(role)
        .execute(&self.pool)
        .await
        .expect("create enrollment");
        id
    }

    /// A new custom role holding only `course.participate`.
    async fn participant_role(&self) -> Uuid {
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: format!("Participant of {}", self.slug),
            permissions: vec![CourseParticipate::KEY.to_owned()],
        };
        roles::create_custom(&self.pool, scope(self.id), &role, now(), Actor::Instance)
            .await
            .expect("create role")
            .id
    }

    /// A new user enrolled in `course` with [`Self::participant_role`]: the user's id.
    async fn learner(&self, course: Uuid) -> Uuid {
        let (_, user) = self.user().await;
        let role = self.participant_role().await;
        self.enroll(user, role, course).await;
        user
    }

    /// A new user enrolled in `course` with [`Self::participant_role`]: the enrollment's id.
    async fn enrollment(&self, course: Uuid) -> Uuid {
        let (_, user) = self.user().await;
        let role = self.participant_role().await;
        self.enroll(user, role, course).await
    }

    /// A new custom role with no permissions and no assignment.
    async fn custom_role(&self) -> Uuid {
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Target role".into(),
            permissions: Vec::new(),
        };
        roles::create_custom(&self.pool, scope(self.id), &role, now(), Actor::Instance)
            .await
            .expect("create role")
            .id
    }

    /// A signed-in Organization admin at organization scope: their `Cookie` header value and id.
    async fn admin(&self) -> (String, Uuid) {
        let (cookie, user) = self.user().await;
        let role = sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
            .bind("org_admin")
            .fetch_one(&self.pool)
            .await
            .expect("a built-in role");
        self.assign(user, role, At::Org).await;
        (cookie, user)
    }

    /// Link `user` to the organization's local provider, as an enrollment does.
    async fn local_identity(&self, user: Uuid) {
        let local = stepclimb_core::identity_providers::get_by_key(
            &self.pool,
            scope(self.id),
            stepclimb_core::local_sign_in::PROVIDER_KEY,
        )
        .await
        .expect("read the local row")
        .expect("the local row");
        let identity = NewIdentity {
            user_id: user,
            provider_id: local.id,
            subject: user.to_string(),
            attributes: serde_json::Map::new(),
        };
        identities::link(
            &self.pool,
            scope(self.id),
            &identity,
            now(),
            Actor::Instance,
        )
        .await
        .expect("link the local identity");
    }

    /// A new passkey of `user` with a placeholder credential, which the management routes never
    /// read: its id.
    async fn passkey(&self, user: Uuid) -> Uuid {
        let credential = serde_json::json!({ "placeholder": true });
        let credential_id = new_id().as_bytes().to_vec();
        let new = stepclimb_core::passkeys::NewPasskey {
            user_id: user,
            credential_id: &credential_id,
            credential: &credential,
            name: "Suite passkey",
        };
        stepclimb_core::passkeys::create(&self.pool, scope(self.id), new, now(), Actor::Instance)
            .await
            .expect("store a passkey")
            .id
    }

    async fn holder_of_builtin(&self, key: &str, at: At) -> String {
        let (cookie, user) = self.user().await;
        let role = sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
            .bind(key)
            .fetch_one(&self.pool)
            .await
            .expect("a built-in role");
        self.assign(user, role, at).await;
        cookie
    }
}

// The runner on test-only routes, one per target kind, and two broken ones (T15).

async fn org_route(_: Require<OrgManage>) -> &'static str {
    "ok"
}

async fn rename_account(
    require: Require<AccountManage, AccountTarget>,
    State(state): State<AppState>,
) -> Result<StatusCode, ApiError> {
    let scope = require.principal().scope();
    accounts::rename(
        &state.db,
        scope,
        require.target().id,
        "Renamed",
        now(),
        Actor::Instance,
    )
    .await
    .map_err(|error| ApiError::internal(&error))?;
    Ok(StatusCode::OK)
}

async fn delete_account(
    require: Require<AccountManage, AccountParentTarget>,
    State(state): State<AppState>,
) -> Result<StatusCode, ApiError> {
    let scope = require.principal().scope();
    accounts::delete(
        &state.db,
        scope,
        require.target().id,
        UNIX_EPOCH,
        Actor::Instance,
    )
    .await
    .map_err(|error| ApiError::internal(&error))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_roles(require: Require<RoleView, AnyScope>) -> String {
    format!("{:?}", require.grants().scopes_with::<RoleView>())
}

#[derive(Deserialize)]
struct AssignBody {
    account_id: Uuid,
}

async fn assign_role(
    require: Require<RoleAssign, AnyScope>,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<AssignBody>,
) -> Result<StatusCode, ApiError> {
    let at = ScopeId::Account(body.account_id);
    require
        .grants()
        .require_at::<RoleAssign>(&state.db, at)
        .await?;
    Ok(StatusCode::CREATED)
}

async fn me(principal: Authenticated) -> String {
    principal.user_id.to_string()
}

/// No `Require`, and an update that ignores the organization.
async fn open_rename(State(state): State<AppState>, ApiPath(id): ApiPath<Uuid>) -> StatusCode {
    sqlx::query("UPDATE accounts SET name = 'Renamed' WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await
        .expect("update");
    StatusCode::OK
}

/// Signs every other user out, in every organization.
async fn end_other_sessions(principal: Authenticated, State(state): State<AppState>) -> StatusCode {
    sqlx::query("DELETE FROM sessions WHERE user_id <> $1")
        .bind(principal.user_id)
        .execute(&state.db)
        .await
        .expect("delete");
    StatusCode::NO_CONTENT
}

/// Every user's name, in every organization.
async fn every_name(_: Authenticated, State(state): State<AppState>) -> String {
    let names: Vec<String> = sqlx::query_scalar("SELECT display_name FROM users")
        .fetch_all(&state.db)
        .await
        .expect("select");
    names.join(",")
}

/// [`rename_account`] behind the `custom_roles` feature.
async fn gated_rename(
    _: Require<CustomRoles>,
    require: Require<AccountManage, AccountTarget>,
    state: State<AppState>,
) -> Result<StatusCode, ApiError> {
    rename_account(require, state).await
}

/// Removes one of the caller's own identities.
async fn delete_mine(
    principal: Authenticated,
    State(state): State<AppState>,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    identities::unlink(
        &state.db,
        principal.scope(),
        principal.user_id,
        id,
        now(),
        Actor::Instance,
    )
    .await
    .map_err(|_| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "identity.not_found",
            "No such identity.",
        )
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// Removes any identity by id, whoever and wherever its user is.
async fn delete_anyones(
    _: Authenticated,
    State(state): State<AppState>,
    ApiPath(id): ApiPath<Uuid>,
) -> StatusCode {
    sqlx::query("DELETE FROM identities WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await
        .expect("delete");
    StatusCode::NO_CONTENT
}

/// Checks `account.view` where its entry says `account.manage`.
async fn wrong_permission(_: Require<AccountView, AccountTarget>) -> StatusCode {
    StatusCode::OK
}

fn fixture_app(world: &World) -> Router {
    Router::new()
        .route("/t/org", get(org_route))
        .route(
            "/t/accounts/{account_id}",
            patch(rename_account).delete(delete_account),
        )
        .route("/t/roles", get(list_roles))
        .route("/t/assignments", post(assign_role))
        .route("/t/me", get(me))
        .route("/t/open/{account_id}", patch(open_rename))
        .route("/t/wrong/{account_id}", patch(wrong_permission))
        .route("/t/gated/{account_id}", patch(gated_rename))
        .route("/t/sessions", axum::routing::delete(end_other_sessions))
        .route("/t/names", get(every_name))
        .route("/t/mine/{id}", axum::routing::delete(delete_mine))
        .route("/t/anyones/{id}", axum::routing::delete(delete_anyones))
        .with_state(world.state())
}

fn fixture_op(method: Method, path: &str) -> Operation {
    Operation {
        id: format!("{method} {path}"),
        method,
        path: path.to_owned(),
    }
}

fn at_org(_: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: None,
            at: At::Org,
        }
    })
}

fn tower_in_path(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: vec![("account_id", org.tower)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

/// Ground, checked at its parent Tower.
fn ground_under_tower(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: vec![("account_id", org.ground)],
            body: None,
            at: At::Account(org.tower),
        }
    })
}

fn tower_in_body(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        Call {
            params: Vec::new(),
            body: Some(serde_json::json!({ "account_id": org.tower })),
            at: At::Account(org.tower),
        }
    })
}

const ACCOUNT_NOT_FOUND: Target = Target::Scoped {
    not_found: "account.not_found",
    cross_org: "account.not_found",
};

const fn permission(key: &'static str, target: Target, setup: Setup) -> Access {
    Access::Permission {
        key,
        target,
        extra: &[],
        feature: None,
        feature_by_target: false,
        denied_also: &[],
        setup,
    }
}

/// `access`, a [`permission`] entry, for a route that also takes `Require<F>` with
/// `F::KEY == feature`.
const fn gated(feature: &'static str, access: Access) -> Access {
    match access {
        Access::Permission {
            key,
            target,
            extra,
            setup,
            ..
        } => Access::Permission {
            key,
            target,
            extra,
            feature: Some(feature),
            feature_by_target: false,
            denied_also: &[],
            setup,
        },
        Access::AuthOnly(_) | Access::Own { .. } | Access::Mine(_) => {
            panic!("a feature entry needs a permission")
        }
    }
}

/// [`gated`], for a route whose target decides which feature it checks, after the target is
/// found: the provider routes, where an `oidc` provider needs `oidc_providers` and an `oauth2`
/// one `oauth2_providers` (spec 06 I5). The entry names the feature of the target its setup
/// builds.
const fn gated_by_target(feature: &'static str, access: Access) -> Access {
    match gated(feature, access) {
        Access::Permission {
            key,
            target,
            extra,
            feature,
            setup,
            ..
        } => Access::Permission {
            key,
            target,
            extra,
            feature,
            feature_by_target: true,
            denied_also: &[],
            setup,
        },
        Access::AuthOnly(_) | Access::Own { .. } | Access::Mine(_) => {
            panic!("a feature entry needs a permission")
        }
    }
}

const EVERY_CASE: [&str; 6] = [
    "401",
    "403",
    "404",
    "2xx",
    "every feature off",
    "cross-organization",
];
const NO_404: [&str; 5] = [
    "401",
    "403",
    "2xx",
    "every feature off",
    "cross-organization",
];

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_runner_passes_a_correct_route_of_each_target_kind(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let cases: [(Method, &str, Access, &[&str]); 6] = [
        (
            Method::GET,
            "/t/org",
            permission(OrgManage::KEY, Target::Org, at_org),
            &NO_404,
        ),
        (
            Method::PATCH,
            "/t/accounts/{account_id}",
            permission(AccountManage::KEY, ACCOUNT_NOT_FOUND, tower_in_path),
            &EVERY_CASE,
        ),
        (
            Method::DELETE,
            "/t/accounts/{account_id}",
            permission(AccountManage::KEY, ACCOUNT_NOT_FOUND, ground_under_tower),
            &EVERY_CASE,
        ),
        (
            Method::GET,
            "/t/roles",
            permission(RoleView::KEY, Target::AnyScope, at_org),
            &NO_404,
        ),
        (
            Method::POST,
            "/t/assignments",
            permission(RoleAssign::KEY, ACCOUNT_NOT_FOUND, tower_in_body),
            &EVERY_CASE,
        ),
        (
            Method::GET,
            "/t/me",
            Access::AuthOnly(at_org),
            &["401", "2xx", "every feature off", "cross-organization"],
        ),
    ];
    for (method, path, access, expected) in cases {
        let op = fixture_op(method, path);
        let report = run(&world, &app, &op, &access).await;
        assert!(report.failures.is_empty(), "{:#?}", report.failures);
        assert_eq!(report.ran, expected, "{}", op.id);
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_runner_passes_a_route_that_checks_its_feature(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let op = fixture_op(Method::PATCH, "/t/gated/{account_id}");
    let access = gated(
        CustomRoles::KEY,
        permission(AccountManage::KEY, ACCOUNT_NOT_FOUND, tower_in_path),
    );
    let report = run(&world, &app, &op, &access).await;
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        report.ran,
        [
            "401",
            "feature off",
            "feature off, unknown ids",
            "403",
            "404",
            "2xx",
            "cross-organization"
        ]
    );
}

/// Two identities of `owner`, the second named as the fixture routes' `{id}`.
fn fixture_identity(org: &Org, owner: Uuid) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async move {
        Call {
            params: vec![("id", org.two_identities(owner).await)],
            body: None,
            at: At::Org,
        }
    })
}

const OWN_IDENTITY: Access = Access::Own {
    not_found: "identity.not_found",
    setup: fixture_identity,
};

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_runner_passes_a_route_on_the_callers_own_row(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let op = fixture_op(Method::DELETE, "/t/mine/{id}");
    let report = run(&world, &app, &op, &OWN_IDENTITY).await;
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        report.ran,
        [
            "401",
            "2xx",
            "every feature off",
            "other-user",
            "cross-organization"
        ]
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_route_without_its_feature_check_fails_the_feature_off_cases(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let op = fixture_op(Method::PATCH, "/t/accounts/{account_id}");
    let access = gated(
        CustomRoles::KEY,
        permission(AccountManage::KEY, ACCOUNT_NOT_FOUND, tower_in_path),
    );
    let report = run(&world, &app, &op, &access).await;
    assert_eq!(
        failed_cases(&report),
        ["feature off", "feature off, unknown ids"],
        "{report:#?}"
    );
    assert!(
        report.failures[0].contains("the organization's rows changed"),
        "{report:#?}"
    );
    assert!(
        report.failures[1].contains("account.not_found"),
        "{report:#?}"
    );
}

/// A route that checks a feature its entry does not declare fails only its every-feature-off
/// case: with every feature on, the other cases cannot see the check.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_undeclared_feature_check_fails_the_every_feature_off_case(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let op = fixture_op(Method::PATCH, "/t/gated/{account_id}");
    let access = permission(AccountManage::KEY, ACCOUNT_NOT_FOUND, tower_in_path);
    let report = run(&world, &app, &op, &access).await;
    assert_eq!(report.ran, EVERY_CASE);
    assert_eq!(failed_cases(&report), ["every feature off"], "{report:#?}");
    assert!(
        report.failures[0].contains("entitlement.feature_unavailable"),
        "{report:#?}"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_route_on_anyones_row_fails_the_other_user_and_cross_organization_cases(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let op = fixture_op(Method::DELETE, "/t/anyones/{id}");
    let report = run(&world, &app, &op, &OWN_IDENTITY).await;
    assert_eq!(
        failed_cases(&report),
        ["other-user", "cross-organization"],
        "{report:#?}"
    );
    assert!(
        report.failures[1].contains("org B's rows changed"),
        "{report:#?}"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_unknown_feature_key_fails_the_entry(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let op = fixture_op(Method::PATCH, "/t/gated/{account_id}");
    let access = gated(
        "teleportation",
        permission(AccountManage::KEY, ACCOUNT_NOT_FOUND, tower_in_path),
    );
    let report = run(&world, &app, &op, &access).await;
    assert_eq!(report.ran, ["catalog"]);
    assert_eq!(report.failures.len(), 1);
}

/// Organization-only permissions have no 404 case: no account grant can hold them.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_organization_only_permission_has_no_404_case(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let op = fixture_op(Method::GET, "/t/org");
    let access = permission(OrgManage::KEY, ACCOUNT_NOT_FOUND, at_org);
    let report = run(&world, &app, &op, &access).await;
    assert_eq!(report.ran, NO_404);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t15_a_route_without_require_fails_its_401_and_cross_organization_cases(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let op = fixture_op(Method::PATCH, "/t/open/{account_id}");
    let access = permission(AccountManage::KEY, ACCOUNT_NOT_FOUND, tower_in_path);
    let report = run(&world, &app, &op, &access).await;
    let failed = failed_cases(&report);
    assert_eq!(
        failed,
        ["401", "403", "404", "cross-organization"],
        "{report:#?}"
    );
    assert!(
        report.failures[3].contains("org B's rows changed"),
        "{report:#?}"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t15_a_route_checking_another_permission_fails_its_2xx_case(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let op = fixture_op(Method::PATCH, "/t/wrong/{account_id}");
    let access = permission(AccountManage::KEY, ACCOUNT_NOT_FOUND, tower_in_path);
    let report = run(&world, &app, &op, &access).await;
    let failed = failed_cases(&report);
    assert_eq!(failed, ["403", "2xx"], "{report:#?}");
}

/// The case names in `report`'s failures, in order.
fn failed_cases(report: &Report) -> Vec<&str> {
    report
        .failures
        .iter()
        .map(|f| {
            let (_, rest) = f.split_once(") ").expect("the operation, then a case");
            rest.split_once(": ").expect("a case, then why").0
        })
        .collect()
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_auth_only_route_ending_org_bs_sessions_fails_the_cross_organization_case(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let op = fixture_op(Method::DELETE, "/t/sessions");
    let report = run(&world, &app, &op, &Access::AuthOnly(at_org)).await;
    assert_eq!(failed_cases(&report), ["cross-organization"], "{report:#?}");
    assert!(
        report.failures[0].ends_with("org B's rows changed"),
        "{report:#?}"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_auth_only_route_answering_org_bs_names_fails_the_cross_organization_case(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let op = fixture_op(Method::GET, "/t/names");
    let report = run(&world, &app, &op, &Access::AuthOnly(at_org)).await;
    assert_eq!(failed_cases(&report), ["cross-organization"], "{report:#?}");
    assert!(
        report.failures[0].contains("the response holds org B's value \"org-"),
        "{report:#?}"
    );
}

/// Nothing the caller owns: the `Mine` runner's setup for the fixture routes.
fn nothing_of_caller(_: &Org, _: Uuid) -> Pin<Box<dyn Future<Output = Mine> + Send + '_>> {
    Box::pin(async {
        Mine::call(Call {
            params: Vec::new(),
            body: None,
            at: At::Org,
        })
    })
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_runner_passes_a_route_that_needs_the_callers_own_rows(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let op = fixture_op(Method::GET, "/t/me");
    let report = run(&world, &app, &op, &Access::Mine(nothing_of_caller)).await;
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        report.ran,
        ["401", "2xx", "every feature off", "cross-organization"]
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_mine_route_answering_org_bs_names_fails_the_cross_organization_case(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let op = fixture_op(Method::GET, "/t/names");
    let report = run(&world, &app, &op, &Access::Mine(nothing_of_caller)).await;
    assert_eq!(failed_cases(&report), ["cross-organization"], "{report:#?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_table_scoped_by_neither_rule_fails_the_snapshot(pool: PgPool) {
    let world = World::new(pool).await;
    let org = world.org().await;
    assert!(snapshot(&world.pool, org.id).await.is_ok());
    sqlx::query("CREATE TABLE stray_notes (id uuid PRIMARY KEY, user_id uuid)")
        .execute(&world.pool)
        .await
        .expect("create table");
    let error = snapshot(&world.pool, org.id)
        .await
        .expect_err("an unscoped table");
    assert!(error.ends_with(r#"["stray_notes"]"#), "{error}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_unknown_permission_key_fails_the_entry(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let op = fixture_op(Method::GET, "/t/org");
    let report = run(
        &world,
        &app,
        &op,
        &permission("org.everything", Target::Org, at_org),
    )
    .await;
    assert_eq!(report.ran, ["catalog"]);
    assert_eq!(report.failures.len(), 1);
}

// The test-support organization header.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_test_header_picks_one_of_several_organizations(pool: PgPool) {
    let world = World::new(pool).await;
    let app = fixture_app(&world);
    let a = world.org().await;
    let b = world.org().await;
    let (cookie, user) = b.user().await;
    let me = |org: Option<Uuid>| {
        let mut request = Request::builder()
            .uri("/t/me")
            .header(header::COOKIE, &cookie);
        if let Some(org) = org {
            request = request.header(TEST_ORG_HEADER, org.to_string());
        }
        let app = app.clone();
        async move {
            let response = app
                .oneshot(request.body(Body::empty()).expect("a request"))
                .await
                .expect("the router never fails");
            let status = response.status();
            let headers = response.headers().clone();
            let body = response
                .into_body()
                .collect()
                .await
                .expect("body")
                .to_bytes();
            Answer {
                status,
                headers,
                body: String::from_utf8_lossy(&body).into_owned(),
                head: false,
            }
        }
    };
    let answer = me(Some(b.id)).await;
    assert_eq!(
        (answer.status, answer.body),
        (StatusCode::OK, user.to_string())
    );
    // Org B's session is not one of org A's (spec 08 B6).
    me(Some(a.id))
        .await
        .expect(StatusCode::UNAUTHORIZED, "auth.unauthenticated")
        .unwrap();
    me(Some(new_id()))
        .await
        .expect(StatusCode::NOT_FOUND, "org.not_found")
        .unwrap();
    // Without the header the production rule applies: two organizations, none is picked.
    me(None)
        .await
        .expect(StatusCode::NOT_FOUND, "org.not_found")
        .unwrap();
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_malformed_test_header_names_no_organization(pool: PgPool) {
    let world = World::new(pool).await;
    let org = world.org().await;
    let (cookie, _) = org.user().await;
    let request = Request::builder()
        .uri("/t/me")
        .header(header::COOKIE, cookie)
        .header(TEST_ORG_HEADER, "not-a-uuid")
        .body(Body::empty())
        .expect("a request");
    let response = fixture_app(&world)
        .oneshot(request)
        .await
        .expect("the router never fails");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    Answer {
        status,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
        head: false,
    }
    .expect(StatusCode::NOT_FOUND, "org.not_found")
    .unwrap();
}
