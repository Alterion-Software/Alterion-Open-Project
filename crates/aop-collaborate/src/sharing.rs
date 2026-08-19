//! Who may be let into a plan, decided without a database and without the
//! identity provider in sight.
//!
//! The awkward fact this works around is that nobody knows anybody else's
//! `sub`. It is a UUID the identity provider minted, it is shown in no
//! interface, and an endpoint that took one would be asking a person to look
//! up something they have no way to look up. People know addresses.
//!
//! Turning an address into a subject on this side would mean asking the
//! provider "who is ada@example.com". An endpoint that answers that tells
//! anybody who can call it whether an account exists, one address at a time,
//! which is a user enumeration oracle wearing a convenience feature's clothes.
//! So it is not built and it is not asked for.
//!
//! What happens instead is that the address waits:
//!
//! ```text
//!   owner: invite ada@example.com as editor
//!        -> a pending invite. Nothing has been looked up and nobody has been
//!           told whether that address belongs to anyone.
//!
//!   Ada:   here is my token, I am claiming
//!        -> the provider is asked which address *this token* belongs to,
//!           which is a question about the caller and about nobody else.
//!        -> the two agree: she becomes a member and the invite is consumed.
//! ```
//!
//! Three properties fall out of that shape and are the reason for it.
//!
//! Nothing is ever looked up by address, so no request to this server can be
//! used to probe who exists. The invitee proves who they are with their own
//! token rather than being vouched for by the owner, so an invite sent to the
//! wrong address grants nothing to whoever holds the wrong address. And an
//! invite is single use because claiming deletes the row, so somebody removed
//! from a plan cannot walk back in through the invite they came by.

use crate::entity::role;
use crate::error::SyncError;

/// The longest address this server will store.
///
/// RFC 5321 puts the limit at 254 octets for the whole path. Refusing longer
/// ones is not validation for its own sake: an address is a primary key here,
/// and an unbounded one is a row somebody else pays for.
const LONGEST: usize = 254;

/// An address, normalised, or nothing if it is not one.
///
/// Trimmed and lower cased, because the owner types it and the provider says
/// it and neither of them agrees with the other about capitalisation. The
/// local part of an address is technically case sensitive; in practice no
/// provider treats it that way, and an invite to `Ada@Example.com` that never
/// matches `ada@example.com` is a feature that appears to work and does not.
///
/// Deliberately not a full grammar. The only thing that has to be true here is
/// that this is one address and not a list, a header, or a sentence: whether
/// it can actually receive mail is answered by nothing ever arriving.
pub fn address(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > LONGEST {
        return None;
    }
    // Whitespace and controls are how one field becomes two: a newline in a
    // stored address is a header injection waiting for the day something here
    // sends mail.
    if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let (local, domain) = trimmed.split_once('@')?;
    if local.is_empty() || domain.is_empty() || domain.contains('@') {
        return None;
    }
    Some(trimmed.to_lowercase())
}

/// What the identity provider said about the address of whoever is calling.
///
/// A third state for "could not ask" is the whole reason this is an enum
/// rather than an `Option<String>`. A claim that could not be checked and a
/// claim that was checked and refused are different events, and answering the
/// first as the second would tell somebody they were not invited when the
/// truth is that nobody knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presented {
    /// An address the provider says it has verified.
    Verified(String),
    /// The provider answered, and its answer is not one to act on.
    Unverified(&'static str),
    /// The provider could not be asked at all.
    Unavailable(String),
}

/// Read the provider's answer into the three states that matter.
///
/// The verified flag is not a formality. Without it, anybody holding an
/// account on this provider claims any invite by writing the invited address
/// into their own profile, and the whole scheme is decoration. A provider that
/// does not send the flag is treated exactly like one that sends `false`,
/// because "this provider has not said whether it checked" and "this provider
/// did not check" are the same amount of evidence.
pub fn presented(email: Option<&str>, verified: Option<bool>) -> Presented {
    let Some(address) = email.and_then(address) else {
        return Presented::Unverified(
            "your identity provider did not say which address this account uses, so there \
             is nothing to match an invitation against",
        );
    };
    match verified {
        Some(true) => Presented::Verified(address),
        _ => Presented::Unverified(
            "your identity provider has not confirmed that this address belongs to this \
             account, and an unconfirmed address is one anybody could have typed. Confirm \
             your address with your provider and try again",
        ),
    }
}

/// An invitation, as the decision needs it.
///
/// The row it came from carries who sent it and when, which matter to whoever
/// reads the list and not at all to whether it may be claimed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offered {
    pub email: String,
    pub role: String,
}

/// What to do about somebody presenting themselves for a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    /// They were already in. Nothing is written, and their role is unchanged:
    /// there is no path in this server that changes a role once it is held,
    /// which is what stops a stale invite quietly demoting somebody.
    Already(String),
    /// The invitation matches. Write the membership, delete the invitation.
    Grant { role: String, email: String },
    /// Nothing here for this person. Answered as "not found", exactly as a
    /// plan that does not exist is, so that a claim cannot be used to find out
    /// which plan ids are real.
    NoInvite,
    /// The provider's answer is not one to act on, and the reason is about the
    /// caller's own account rather than about this plan, so saying it reveals
    /// nothing about the plan.
    NotVerified(&'static str),
    /// The provider could not be asked. Fails closed, and says which of the
    /// two it is.
    CannotCheck(String),
}

/// The whole of the decision.
///
/// The order is what makes it safe. Membership first, because a member already
/// knows the plan exists and can be answered without asking the provider
/// anything. Then the caller's own address, before the invitation is looked at
/// at all: a refusal at that point is about the caller, so it is the same
/// refusal whether or not the plan exists. Only then the invitation, whose
/// absence is a bare "not found".
pub fn decide(held: Option<&str>, presented: &Presented, invite: Option<&Offered>) -> Claim {
    if let Some(role) = held {
        return Claim::Already(role.to_string());
    }
    let address = match presented {
        Presented::Verified(address) => address,
        Presented::Unverified(why) => return Claim::NotVerified(why),
        Presented::Unavailable(why) => return Claim::CannotCheck(why.clone()),
    };

    let Some(invite) = invite else {
        return Claim::NoInvite;
    };
    // The lookup was by this address, so this can only disagree if the lookup
    // was written wrongly. It is checked anyway, because the cost is a string
    // comparison and the thing being prevented is somebody else's invitation
    // being handed to whoever asked.
    if &invite.email != address {
        return Claim::NoInvite;
    }
    // A stored role that is not one an invitation may carry means the row did
    // not come through the invite endpoint. Refusing beats guessing which
    // access was meant.
    match role::invitable(&invite.role) {
        Some(role) => Claim::Grant {
            role: role.to_string(),
            email: address.clone(),
        },
        None => Claim::NoInvite,
    }
}

/// The answer for a caller who has to be the owner and may not be.
///
/// Both halves matter and they are different answers. Somebody who is not a
/// member at all is told the plan is not there, because telling them it is
/// there but not theirs confirms that the id they tried is real. Somebody who
/// *is* a member already knows it is real, so they get the honest refusal.
///
/// The role comes from the caller's row in the database and from nowhere else.
/// Nothing in a request says what the person asking is allowed to do.
pub fn manager(held: Option<String>) -> Result<String, SyncError> {
    let held = held.ok_or(SyncError::NotFound)?;
    if held != role::OWNER {
        return Err(SyncError::Forbidden);
    }
    Ok(held)
}

/// Whether this member may be removed.
///
/// The owner may not, by anybody, including themselves. A plan whose owner has
/// been removed is a plan nobody can share, nobody can delete, and whose
/// `owner_subject` names an account with no way back in. Deleting the plan is
/// the operation that means "I am done with this", and it already exists.
///
/// A bad request rather than a refusal, because it is not about who is asking:
/// there is no caller for whom this would have worked.
pub fn removable(owner_subject: &str, target: &str) -> Result<(), SyncError> {
    if owner_subject == target {
        return Err(SyncError::BadRequest(
            "the owner cannot be removed from their own plan. Delete the plan instead, or \
             pass it on first"
                .into(),
        ));
    }
    Ok(())
}
