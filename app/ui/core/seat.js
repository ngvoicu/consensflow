/**
 * The seat a participant holds on the board, and the questions the page asks of it. The one
 * place the page compares a role with a word of its own: every other place asks the chief,
 * a coordinator or a member, as `cf_proto::seat::Seat` does for the Rust side
 * (`crates/cf-proto/tests/seat_comparisons.rs` holds both to it).
 */

const SEATS = new Set(['human', 'chief', 'lead'])

/** The seat of a role word: `human`, `chief`, `lead`, or `member` for any other. */
export const seatOf = (role) => (SEATS.has(role) ? role : 'member')

/** The project's one chief. */
export const isChief = (participant) => seatOf(participant.role) === 'chief'

/** The chief, and a lead where there is one. */
export const coordinates = (participant) => ['chief', 'lead'].includes(seatOf(participant.role))

/** A member of the staff, by its seat. */
export const holdsMemberSeat = (participant) => seatOf(participant.role) === 'member'
