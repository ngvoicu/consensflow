//! The environment a window's program is started with: the command's whole, every value as
//! the system keeps it, Unicode or not, as portable-pty passed it on.
//!
//! portable-pty's `CommandBuilder` holds its variables in a map of its own, and lists only
//! those whose name and value are Unicode (`iter_full_env_as_str`); a value is had by its
//! name (`get_env`), Unicode or not. So the names are read from the builder's own map,
//! through the form portable-pty gives it to serde (its `serde_support` feature: the map
//! `envs`, each entry's name in `preferred_key`, a Windows string as
//! `Windows([u16…])`), and each value is asked of the builder by its name. Nothing else is
//! read of the form: no value goes through it. What the builder holds is what it was made
//! with (the host's environment and the registry's) with the frame's overlay and removals
//! applied, so the block is its own, in its own order (by its key for each name: the name in
//! lower case, or as it is where it is not Unicode).
//!
//! Where the form cannot be read as portable-pty 0.9 writes it (a later version that wrote
//! another), the names are the builder's list, and a variable whose value is not Unicode is
//! left out: the pane host's log says so once, with why the form could not be read and
//! never a name's value (which names were left out the form would have said). The tests
//! pin the form (`tests.rs`), so an upgrade that changes it fails there first.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::sync::{Mutex, PoisonError};

use portable_pty::CommandBuilder;
use serde::ser::{self, Impossible, Serialize, SerializeMap, SerializeSeq, SerializeStruct};

/// The command's whole environment, as `NAME=value` wide strings in the builder's order,
/// ended by an empty one, and by a second where there is none.
pub(super) fn environment_block(command: &CommandBuilder) -> Vec<u16> {
    block_of(command, names_of(command))
}

/// The block of `command`'s variables named `names`, or of those it lists where its names
/// could not be read.
pub(super) fn block_of(command: &CommandBuilder, names: Result<Vec<OsString>, Unread>) -> Vec<u16> {
    let names = names.unwrap_or_else(|unread| {
        say_once(format!(
            "the environment of a window's program could not be read as portable-pty 0.9 \
             writes it ({unread}): only its variables whose name and value are Unicode are \
             passed on"
        ));
        command
            .iter_full_env_as_str()
            .map(|(name, _)| OsString::from(name))
            .collect()
    });
    let mut block = Vec::new();
    for name in &names {
        let Some(value) = command.get_env(name) else {
            continue;
        };
        block.extend(name.encode_wide());
        block.push(u16::from(b'='));
        block.extend(value.encode_wide());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

/// The names of `builder`'s variables, in its order, as its serde form has them; or why that
/// form could not be read.
pub(super) fn names_of(builder: &impl Serialize) -> Result<Vec<OsString>, Unread> {
    let mut found = Found::default();
    builder.serialize(Walk {
        found: &mut found,
        at: Place::Builder,
    })?;
    if !found.envs {
        return Err(Unread("it has no `envs`".into()));
    }
    Ok(found.names)
}

/// Says `line` in the pane host's log (its error output, which the app keeps), once for the
/// life of the host.
fn say_once(line: String) {
    static SAID: Mutex<Option<HashSet<String>>> = Mutex::new(None);
    let mut said = SAID.lock().unwrap_or_else(PoisonError::into_inner);
    if !said.get_or_insert_with(HashSet::new).insert(line.clone()) {
        return;
    }
    drop(said);
    say(&line);
}

#[cfg(not(test))]
#[allow(clippy::print_stderr)] // The pane host's log is its error output.
fn say(line: &str) {
    eprintln!("consensflow-bridge: {line}");
}

#[cfg(test)]
pub(super) static SAID: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[cfg(test)]
fn say(line: &str) {
    SAID.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(line.to_owned());
}

/// Why the builder's serde form could not be read.
#[derive(Debug)]
pub(super) struct Unread(pub(super) String);

impl fmt::Display for Unread {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Unread {}

impl ser::Error for Unread {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self(message.to_string())
    }
}

/// What the walk of the form found: whether it had the map, the names in it, and the name
/// being read.
#[derive(Default)]
struct Found {
    envs: bool,
    names: Vec<OsString>,
    units: Vec<u16>,
}

/// Where in the form a walk is.
#[derive(Clone, Copy)]
enum Place {
    /// The builder: a struct, whose field `envs` is the map.
    Builder,
    /// The map, whose values are the entries.
    Envs,
    /// An entry: a struct, whose field `preferred_key` is the name.
    Entry,
    /// A name: a Windows string, `Windows([u16…])`.
    Name,
    /// The units of a name.
    Units,
}

/// A serializer that reads only the names of the form, and refuses whatever is not where
/// portable-pty 0.9 puts it.
struct Walk<'a> {
    found: &'a mut Found,
    at: Place,
}

impl Walk<'_> {
    fn unexpected(&self, what: &str) -> Unread {
        let place = match self.at {
            Place::Builder => "the builder",
            Place::Envs => "its `envs`",
            Place::Entry => "an entry of its `envs`",
            Place::Name => "an entry's `preferred_key`",
            Place::Units => "a name's units",
        };
        Unread(format!("{place} is {what}"))
    }
}

macro_rules! refused {
    ($($method:ident($($argument:ty),*)),* $(,)?) => {
        $(
            fn $method(self, $(_: $argument),*) -> Result<(), Unread> {
                Err(self.unexpected(stringify!($method)))
            }
        )*
    };
}

impl<'a> ser::Serializer for Walk<'a> {
    type Ok = ();
    type Error = Unread;
    type SerializeSeq = Self;
    type SerializeTuple = Impossible<(), Unread>;
    type SerializeTupleStruct = Impossible<(), Unread>;
    type SerializeTupleVariant = Impossible<(), Unread>;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Impossible<(), Unread>;

    refused!(
        serialize_bool(bool),
        serialize_i8(i8),
        serialize_i16(i16),
        serialize_i32(i32),
        serialize_i64(i64),
        serialize_u8(u8),
        serialize_u32(u32),
        serialize_u64(u64),
        serialize_f32(f32),
        serialize_f64(f64),
        serialize_char(char),
        serialize_str(&str),
        serialize_bytes(&[u8]),
        serialize_none(),
        serialize_unit(),
        serialize_unit_struct(&'static str),
        serialize_unit_variant(&'static str, u32, &'static str),
    );

    fn serialize_u16(self, unit: u16) -> Result<(), Unread> {
        match self.at {
            Place::Units => {
                self.found.units.push(unit);
                Ok(())
            }
            _ => Err(self.unexpected("a number")),
        }
    }

    fn serialize_some<T: Serialize + ?Sized>(self, _: &T) -> Result<(), Unread> {
        Err(self.unexpected("an option"))
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: &T,
    ) -> Result<(), Unread> {
        Err(self.unexpected("a newtype"))
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<(), Unread> {
        match (self.at, variant) {
            (Place::Name, "Windows") => {
                self.found.units.clear();
                value.serialize(Walk {
                    found: &mut *self.found,
                    at: Place::Units,
                })?;
                let name = OsString::from_wide(&self.found.units);
                self.found.names.push(name);
                Ok(())
            }
            _ => Err(self.unexpected(&format!("the variant {variant}"))),
        }
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self, Unread> {
        match self.at {
            Place::Units => Ok(self),
            _ => Err(self.unexpected("a sequence")),
        }
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Unread> {
        Err(self.unexpected("a tuple"))
    }

    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Unread> {
        Err(self.unexpected("a tuple struct"))
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Unread> {
        Err(self.unexpected("a tuple variant"))
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self, Unread> {
        match self.at {
            Place::Envs => {
                self.found.envs = true;
                Ok(self)
            }
            _ => Err(self.unexpected("a map")),
        }
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, Unread> {
        match self.at {
            Place::Builder | Place::Entry => Ok(self),
            _ => Err(self.unexpected("a struct")),
        }
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Unread> {
        Err(self.unexpected("a struct variant"))
    }
}

impl SerializeSeq for Walk<'_> {
    type Ok = ();
    type Error = Unread;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, unit: &T) -> Result<(), Unread> {
        unit.serialize(Walk {
            found: &mut *self.found,
            at: Place::Units,
        })
    }

    fn end(self) -> Result<(), Unread> {
        Ok(())
    }
}

impl SerializeMap for Walk<'_> {
    type Ok = ();
    type Error = Unread;

    /// The map's key is the builder's own (the name in lower case): the entry says the name.
    fn serialize_key<T: Serialize + ?Sized>(&mut self, _: &T) -> Result<(), Unread> {
        Ok(())
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, entry: &T) -> Result<(), Unread> {
        entry.serialize(Walk {
            found: &mut *self.found,
            at: Place::Entry,
        })
    }

    fn end(self) -> Result<(), Unread> {
        Ok(())
    }
}

impl SerializeStruct for Walk<'_> {
    type Ok = ();
    type Error = Unread;

    /// Only the map, and each entry's name, are read; the rest (the arguments, the folder,
    /// each entry's value) goes unread.
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        field: &'static str,
        value: &T,
    ) -> Result<(), Unread> {
        let at = match (self.at, field) {
            (Place::Builder, "envs") => Place::Envs,
            (Place::Entry, "preferred_key") => Place::Name,
            _ => return Ok(()),
        };
        value.serialize(Walk {
            found: &mut *self.found,
            at,
        })
    }

    fn end(self) -> Result<(), Unread> {
        Ok(())
    }
}
