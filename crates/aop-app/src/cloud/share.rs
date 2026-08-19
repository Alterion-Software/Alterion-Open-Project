//! The link that points another copy of this application at a plan.
//!
//! One string has to carry two things: which server keeps the plan, and which
//! plan it is. Anything less is not a link, it is an id somebody still has to
//! be told where to use.
//!
//! ```text
//!   aop://collaborate.example.org/plan/0198f0c2-1111-4222-8333-444455556666
//!         \__________________________/      \__________________________/
//!            the server, without a scheme            the plan
//! ```
//!
//! Three things about that shape are deliberate.
//!
//! The scheme is not in the link. It is worked out from the host by the same
//! rule the rest of the application uses: encrypted everywhere except the
//! loopback interface, where there is no network to read a token off. A link
//! that carried its own scheme would be a link that could ask this copy to
//! send an access token over plain HTTP to a host of the sender's choosing.
//!
//! The plan is last, and it is a UUID. Chat windows and mail clients regularly
//! swallow the punctuation at the end of a pasted address, so the last thing
//! in the link is the thing whose alphabet says exactly where it ends.
//!
//! `/plan/` is a fixed separator rather than a query string. It cannot appear
//! inside a UUID, which leaves the whole of the front free for a server that
//! lives under a path prefix, and it survives the escaping that a query string
//! attracts on its way through other people's software.
//!
//! **What a link cannot do.** It does not admit anybody, and it carries no
//! secret that could. It says where a plan is; the server says who may have
//! it. What lets somebody in is an invitation the owner sent to their email
//! address, which their own copy claims on their behalf the first time it
//! tries this link and is turned away. So a link forwarded to a stranger is a
//! link that does nothing for them, whether they were meant to have it or not.
//!
//! Whoever opens one sees the server it names before anything is sent to it,
//! because a link is an instruction from a stranger to go and talk to a host
//! they chose.

/// The scheme this application is registered for.
pub const SCHEME: &str = "aop://";

/// What separates the server from the plan.
const SEPARATOR: &str = "/plan/";

/// Punctuation that ends up stuck on the end of a pasted address.
///
/// Trimmed rather than refused: somebody who wrote "open aop://...id." has
/// written a perfectly good link and a full stop, and telling them their link
/// is malformed would be answering the wrong question.
const STUCK_ON: &[char] = &[
    '.', ',', ';', ':', '!', '?', ')', ']', '}', '>', '"', '\'', '`', '*', '_', '\u{2019}',
];

/// A plan on a server, as a link names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Share {
    /// The server address, scheme and all, ready to be called.
    pub server: String,
    /// The plan's id on that server.
    pub project: String,
}

/// Whether a host is one this machine is talking to itself over.
///
/// The port is not part of the question, so it is cut off first.
fn is_loopback(authority: &str) -> bool {
    let host = match authority.rsplit_once(':') {
        // An IPv6 literal keeps its colons inside the brackets, so a colon
        // after the closing one is the port and any other is part of the host.
        Some((head, _)) if head.ends_with(']') || !head.contains(']') => head,
        _ => authority,
    };
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

/// Write the link for a plan on a server.
///
/// Gives back nothing when either half is missing, because half a link is a
/// string that looks like it works.
pub fn write(server: &str, project: &str) -> Option<String> {
    let project = project.trim();
    if project.is_empty() {
        return None;
    }
    // The scheme comes off before the slashes do, so an address that is
    // nothing but a scheme is left with nothing rather than with a colon.
    let server = server.trim();
    let bare = server
        .strip_prefix("https://")
        .or_else(|| server.strip_prefix("http://"))
        .unwrap_or(server)
        .trim_matches('/');
    if bare.is_empty() {
        return None;
    }
    Some(format!("{SCHEME}{bare}{SEPARATOR}{project}"))
}

/// Read a link, or say nothing about a string that is not one.
///
/// Deliberately strict about the shape and deliberately forgiving about what
/// is stuck to the outside of it. The string arrives from a browser, a chat
/// message or a paste box, so it has been through software that trims, wraps
/// and decorates; what it must not do is arrive as something other than a plan
/// on a server and be treated as one anyway.
pub fn read(text: &str) -> Option<Share> {
    let text = text.trim().trim_end_matches(STUCK_ON);
    // Case insensitive on the scheme alone: a desktop that hands over
    // "AOP://" is following the standard, and everything after the scheme is
    // an address whose case can matter.
    let rest = text
        .get(..SCHEME.len())
        .filter(|head| head.eq_ignore_ascii_case(SCHEME))
        .map(|head| &text[head.len()..])?;

    // The last separator, so a server that lives under a path prefix
    // containing the word plan is still read the way it was written.
    let (bare, project) = rest.rsplit_once(SEPARATOR)?;
    let bare = bare.trim_matches('/');
    let project = project.trim_matches('/');
    if bare.is_empty() || project.is_empty() {
        return None;
    }
    // Nothing but a plan id may follow. A link is a place this copy will send
    // an access token, so the part naming what to ask for is not the place to
    // be generous: anything with a slash, a query or a fragment in it is
    // somebody appending to the link rather than naming a plan.
    if !project
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return None;
    }

    let authority = bare.split('/').next().unwrap_or_default();
    if authority.is_empty() {
        return None;
    }
    let scheme = if is_loopback(authority) {
        "http://"
    } else {
        "https://"
    };

    Some(Share {
        server: format!("{scheme}{bare}"),
        project: project.to_string(),
    })
}

/// Whether a string is worth handing to [`read`] at all.
///
/// Used to tell a link apart from a file path on the command line. The scheme
/// is the whole test, exactly as the coordinator's packaging registered it.
pub fn looks_like_a_link(text: &str) -> bool {
    text.get(..SCHEME.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(SCHEME))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0198f0c2-1111-4222-8333-444455556666";

    #[test]
    fn a_link_carries_the_server_and_the_plan_and_nothing_else() {
        let link = write("https://collaborate.example.org", ID).expect("both halves are there");
        assert_eq!(link, format!("aop://collaborate.example.org/plan/{ID}"));
        let share = read(&link).expect("what was written reads back");
        assert_eq!(share.server, "https://collaborate.example.org");
        assert_eq!(share.project, ID);
    }

    #[test]
    fn a_server_under_a_path_prefix_survives_the_round_trip() {
        let link = write("https://example.org/collab/", ID).expect("a prefix is a real address");
        assert_eq!(link, format!("aop://example.org/collab/plan/{ID}"));
        assert_eq!(read(&link).unwrap().server, "https://example.org/collab");
    }

    #[test]
    fn only_the_loopback_interface_comes_back_as_plain_http() {
        // The link does not get to choose. Anything else would let whoever
        // wrote one ask this copy to put an access token on the wire in the
        // clear, which is the one thing the address rule exists to stop.
        assert_eq!(
            read(&format!("aop://localhost:8090/plan/{ID}")).unwrap().server,
            "http://localhost:8090"
        );
        assert_eq!(
            read(&format!("aop://127.0.0.1:8090/plan/{ID}")).unwrap().server,
            "http://127.0.0.1:8090"
        );
        assert_eq!(
            read(&format!("aop://not-localhost.example.org/plan/{ID}"))
                .unwrap()
                .server,
            "https://not-localhost.example.org"
        );
        assert_eq!(
            write("http://evil.example.org", ID).and_then(|link| read(&link)).unwrap().server,
            "https://evil.example.org"
        );
    }

    #[test]
    fn punctuation_a_chat_window_stuck_on_the_end_is_not_part_of_the_plan() {
        for decorated in [
            format!("aop://sync.example.org/plan/{ID}."),
            format!("aop://sync.example.org/plan/{ID},"),
            format!("(aop://sync.example.org/plan/{ID})"),
            format!("  aop://sync.example.org/plan/{ID}  "),
        ] {
            // The opening bracket is somebody else's problem: what matters is
            // that a full stop at the end never becomes part of the id.
            let share = read(decorated.trim_start_matches('(')).expect("still a link");
            assert_eq!(share.project, ID);
        }
    }

    #[test]
    fn a_scheme_in_any_case_is_still_the_scheme() {
        assert!(looks_like_a_link("AOP://sync.example.org/plan/x"));
        assert_eq!(read(&format!("AOP://sync.example.org/plan/{ID}")).unwrap().project, ID);
    }

    #[test]
    fn anything_that_is_not_a_plan_on_a_server_is_not_read_as_one() {
        assert!(read("aop://sync.example.org").is_none());
        assert!(read(&format!("aop:///plan/{ID}")).is_none());
        assert!(read("aop://sync.example.org/plan/").is_none());
        // A path, a query or a fragment after the id is somebody adding to the
        // link, and what they added is not going to be honoured quietly.
        assert!(read("aop://sync.example.org/plan/one/two").is_none());
        assert!(read(&format!("aop://sync.example.org/plan/{ID}?then=this")).is_none());
        assert!(read(&format!("aop://sync.example.org/plan/{ID}#here")).is_none());
        assert!(read("https://sync.example.org/plan/x").is_none());
        assert!(read("").is_none());
    }

    #[test]
    fn half_a_link_is_not_written_at_all() {
        assert!(write("", ID).is_none());
        assert!(write("https://", ID).is_none());
        assert!(write("https://sync.example.org", "   ").is_none());
    }

    #[test]
    fn a_file_path_is_not_mistaken_for_a_link() {
        assert!(!looks_like_a_link("/home/ada/plans/bridge.aprj"));
        assert!(!looks_like_a_link("C:\\Plans\\bridge.aprj"));
        assert!(!looks_like_a_link("aop"));
    }
}
