//! Request-path classification for the PyPI mount.

use crate::valid;

/// A classified request path (already percent-decoded exactly once).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Route {
    /// `GET /simple/` — the (empty) project list.
    ProjectList,
    /// `GET /simple/{normalized}/` — a project index.
    Project(String),
    /// 301 to the canonical `/simple/{normalized}/` form.
    Redirect(String),
    /// `GET /files/{project}/{fhp_path}` — a distribution file.
    File {
        project: String,
        fhp_path: String,
        filename: String,
    },
    NotFound,
}

/// Classifies a decoded request path into a [`Route`].
pub(crate) fn classify(path: &str) -> Route {
    if let Some(rest) = path.strip_prefix("/simple") {
        return classify_simple(rest);
    }
    if let Some(rest) = path.strip_prefix("/files/") {
        return classify_file(rest);
    }
    Route::NotFound
}

/// Classifies the remainder after `/simple` (empty, `/`, or `/{project}[/]`).
fn classify_simple(rest: &str) -> Route {
    if rest.is_empty() || rest == "/" {
        return Route::ProjectList;
    }
    let Some(rest) = rest.strip_prefix('/') else {
        // e.g. `/simplex` — not under the simple root.
        return Route::NotFound;
    };
    let (name, slashed) = match rest.strip_suffix('/') {
        Some(name) => (name, true),
        None => (rest, false),
    };
    if name.contains('/') || !valid::is_valid_name(name) {
        return Route::NotFound;
    }
    let normalized = valid::normalize(name);
    if !slashed || normalized != name {
        Route::Redirect(normalized)
    } else {
        Route::Project(normalized)
    }
}

/// Classifies the remainder after `/files/` (`{project}/{fhp_path}`).
fn classify_file(rest: &str) -> Route {
    let Some((project, fhp_path)) = rest.split_once('/') else {
        return Route::NotFound;
    };
    // The files route accepts only already-normalized names (no redirects).
    if !valid::is_valid_name(project) || valid::normalize(project) != project {
        return Route::NotFound;
    }
    let Some(filename) = valid::validate_fhp_path(fhp_path) else {
        return Route::NotFound;
    };
    Route::File {
        project: project.to_owned(),
        fhp_path: fhp_path.to_owned(),
        filename: filename.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_simple_routes() {
        assert_eq!(classify("/simple/"), Route::ProjectList);
        assert_eq!(classify("/simple"), Route::ProjectList);
        assert_eq!(
            classify("/simple/requests/"),
            Route::Project("requests".into())
        );
        // No trailing slash -> redirect to the canonical slash form.
        assert_eq!(
            classify("/simple/requests"),
            Route::Redirect("requests".into())
        );
        // Non-normalized name -> redirect to the normalized path.
        assert_eq!(
            classify("/simple/Foo.Bar_baz/"),
            Route::Redirect("foo-bar-baz".into())
        );
    }

    #[test]
    fn classify_rejects_bad_simple_paths() {
        for path in [
            "/simple/../",
            "/simple/./",
            "/simple/.hidden/",
            "/simple/-leading/",
            "/simple/a b/",
            "/simple/a/b/",
            "/simple//",
            "/simplex",
            "/simple/a%2eb/",
        ] {
            assert_eq!(classify(path), Route::NotFound, "path: {path}");
        }
    }

    #[test]
    fn classify_file_routes() {
        assert_eq!(
            classify("/files/foo/packages/aa/bb/cc/foo-1.0.0.whl"),
            Route::File {
                project: "foo".into(),
                fhp_path: "packages/aa/bb/cc/foo-1.0.0.whl".into(),
                filename: "foo-1.0.0.whl".into(),
            }
        );
    }

    #[test]
    fn classify_rejects_bad_file_paths() {
        for path in [
            "/files/Foo/packages/aa/bb/cc/foo-1.0.0.whl", // non-normalized project
            "/files/foo/packages/aa/bb/cc/foo.exe",       // bad extension
            "/files/foo/packages/../bb/cc/foo-1.0.0.whl", // traversal
            "/files/foo/https://evil.com/foo-1.0.0.whl",  // absolute URL smuggle
            "/files/foo",                                 // no tail
            "/files/foo/",                                // empty tail
        ] {
            assert_eq!(classify(path), Route::NotFound, "path: {path}");
        }
    }

    #[test]
    fn classify_everything_else_404() {
        for path in ["/", "/pypi", "/index/foo", "/simpleextra/x", "/files"] {
            assert_eq!(classify(path), Route::NotFound, "path: {path}");
        }
    }
}
