// Tests for the serializer and deserializer are located in tests/serde_/mod.rs.
// They can be run with `cargo test --features serde_tests`.

use serde::de::{Deserializer as SerdeDeserializer, Error as SerdeError, Visitor, SeqVisitor,
                MapVisitor, VariantVisitor, Deserialize, EnumVisitor};
use std::iter::Peekable;

use PlistEvent;

macro_rules! expect {
    ($next:expr, $pat:pat) => {
        match $next {
            Some(Ok(v@$pat)) => v,
            None => return Err(Error::end_of_stream()),
            _ => return Err(Error::syntax(""))
        }
    };
    ($next:expr, $pat:pat => $save:expr) => {
        match $next {
            Some(Ok($pat)) => $save,
            None => return Err(Error::end_of_stream()),
            _ => return Err(Error::syntax(""))
        }
    };
}

macro_rules! try_next {
    ($next:expr) => {
        match $next {
            Some(Ok(v)) => v,
            Some(Err(_)) => return Err(Error::syntax("")),
            None => return Err(Error::end_of_stream())
        }
    }
}

#[derive(Debug)]
pub enum Error {
    None,
}

impl SerdeError for Error {
    fn syntax(_msg: &str) -> Self {
        panic!("stx");
        Error::None
    }

    fn end_of_stream() -> Self {
        panic!("eos");
        Error::None
    }

    fn unknown_field(_field: &str) -> Self {
        panic!("uf");
        Error::None
    }

    fn missing_field(_field: &'static str) -> Self {
        panic!("mf");
        Error::None
    }
}

pub struct Deserializer<I, E>
    where I: IntoIterator<Item = Result<PlistEvent, E>>
{
    events: Peekable<<I as IntoIterator>::IntoIter>,
}

impl<I, E> Deserializer<I, E> where I: IntoIterator<Item = Result<PlistEvent, E>>
{
    pub fn new(iter: I) -> Deserializer<I, E> {
        Deserializer { events: iter.into_iter().peekable() }
    }
}

impl<I, E> SerdeDeserializer for Deserializer<I, E>
    where I: IntoIterator<Item = Result<PlistEvent, E>>
{
    type Error = Error;

    fn visit<V>(&mut self, mut visitor: V) -> Result<V::Value, Self::Error>
        where V: Visitor
    {
        match try_next!(self.events.next()) {
            PlistEvent::StartPlist => panic!(),
            PlistEvent::EndPlist => panic!(),

            PlistEvent::StartArray(len) => {
                visitor.visit_seq(MapSeq::new(self, len.map(|l| l as usize)))
            }
            PlistEvent::EndArray => return Err(Error::syntax("")),

            PlistEvent::StartDictionary(len) => {
                visitor.visit_map(MapSeq::new(self, len.map(|l| l as usize)))
            }
            PlistEvent::EndDictionary => return Err(Error::syntax("")),

            PlistEvent::BooleanValue(v) => visitor.visit_bool(v),
            PlistEvent::DataValue(v) => visitor.visit_byte_buf(v),
            PlistEvent::DateValue(v) => visitor.visit_string(v.to_rfc3339()),
            PlistEvent::IntegerValue(v) if v.is_positive() => visitor.visit_u64(v as u64),
            PlistEvent::IntegerValue(v) => visitor.visit_i64(v as i64),
            PlistEvent::RealValue(v) => visitor.visit_f64(v),
            PlistEvent::StringValue(v) => visitor.visit_string(v),
        }
    }

    fn visit_unit<V>(&mut self, mut visitor: V) -> Result<V::Value, Self::Error>
        where V: Visitor
    {
        expect!(self.events.next(), PlistEvent::StringValue(_));
        visitor.visit_unit()
    }

    fn visit_option<V>(&mut self, mut visitor: V) -> Result<V::Value, Self::Error>
        where V: Visitor
    {
        expect!(self.events.next(), PlistEvent::StartDictionary(_));

        let ret = match try_next!(self.events.next()) {
            PlistEvent::StringValue(ref s) if &s[..] == "None" => {
                let ret = try!(visitor.visit_none());
                expect!(self.events.next(), PlistEvent::StringValue(_));
                ret
            }
            PlistEvent::StringValue(ref s) if &s[..] == "Some" => try!(visitor.visit_some(self)),
            _ => return Err(Error::syntax("")),
        };

        expect!(self.events.next(), PlistEvent::EndDictionary);

        Ok(ret)
    }

    fn visit_newtype_struct<V>(&mut self,
                               _name: &'static str,
                               mut visitor: V)
                               -> Result<V::Value, Self::Error>
        where V: Visitor
    {
        visitor.visit_newtype_struct(self)
    }

    fn visit_enum<V>(&mut self,
                     _enum: &'static str,
                     _variants: &'static [&'static str],
                     mut visitor: V)
                     -> Result<V::Value, Self::Error>
        where V: EnumVisitor
    {
        expect!(self.events.next(), PlistEvent::StartDictionary(_));
        let ret = try!(visitor.visit(&mut *self));
        expect!(self.events.next(), PlistEvent::EndDictionary);
        Ok(ret)
    }
}

impl<I, E> VariantVisitor for Deserializer<I, E> where I: IntoIterator<Item = Result<PlistEvent, E>>
{
    type Error = Error;

    fn visit_variant<V>(&mut self) -> Result<V, Self::Error>
        where V: Deserialize
    {
        <V as Deserialize>::deserialize(self)
    }

    fn visit_unit(&mut self) -> Result<(), Self::Error> {
        <() as Deserialize>::deserialize(self)
    }

    fn visit_newtype<T>(&mut self) -> Result<T, Self::Error>
        where T: Deserialize
    {
        <T as Deserialize>::deserialize(self)
    }

    fn visit_tuple<V>(&mut self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
        where V: Visitor
    {
        <Self as SerdeDeserializer>::visit_tuple(self, len, visitor)
    }

    fn visit_struct<V>(&mut self,
                       fields: &'static [&'static str],
                       visitor: V)
                       -> Result<V::Value, Self::Error>
        where V: Visitor
    {
        let name = "";
        <Self as SerdeDeserializer>::visit_struct(self, name, fields, visitor)
    }
}

struct MapSeq<'a, I, E>
    where E: 'a,
          I: 'a + IntoIterator<Item = Result<PlistEvent, E>>
{
    de: &'a mut Deserializer<I, E>,
    remaining: Option<usize>,
    finished: bool,
}

impl<'a, I, E> MapSeq<'a, I, E>
    where E: 'a,
          I: 'a + IntoIterator<Item = Result<PlistEvent, E>>
{
    fn new(de: &'a mut Deserializer<I, E>, len: Option<usize>) -> MapSeq<'a, I, E> {
        MapSeq {
            de: de,
            remaining: len,
            finished: false,
        }
    }
}

impl<'a, I, E> SeqVisitor for MapSeq<'a, I, E>
    where E: 'a,
          I: 'a + IntoIterator<Item = Result<PlistEvent, E>>
{
    type Error = Error;

    fn visit<T>(&mut self) -> Result<Option<T>, Self::Error>
        where T: Deserialize
    {
        match self.de.events.peek() {
            Some(&Ok(PlistEvent::EndArray)) => {
                self.de.events.next();
                self.finished = true;
                return Ok(None);
            }
            _ => <T as Deserialize>::deserialize(self.de).map(|k| Some(k)),
        }
    }

    fn end(&mut self) -> Result<(), Self::Error> {
        if !self.finished {
            self.finished = true;
            expect!(self.de.events.next(), PlistEvent::EndArray);
        }
        Ok(())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        <Self as MapVisitor>::size_hint(self)
    }
}

impl<'a, I, E> MapVisitor for MapSeq<'a, I, E>
    where E: 'a,
          I: 'a + IntoIterator<Item = Result<PlistEvent, E>>
{
    type Error = Error;

    fn visit_key<K>(&mut self) -> Result<Option<K>, Self::Error>
        where K: Deserialize
    {
        match self.de.events.peek() {
            Some(&Ok(PlistEvent::EndDictionary)) => {
                self.de.events.next();
                self.finished = true;
                return Ok(None);
            }
            _ => <K as Deserialize>::deserialize(self.de).map(|k| Some(k)),
        }
    }

    fn visit_value<V>(&mut self) -> Result<V, Self::Error>
        where V: Deserialize
    {
        <V as Deserialize>::deserialize(self.de)
    }

    fn end(&mut self) -> Result<(), Self::Error> {
        if !self.finished {
            self.finished = true;
            expect!(self.de.events.next(), PlistEvent::EndDictionary);
        }
        Ok(())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self.remaining {
            Some(len) => (len, Some(len)),
            None => (0, None),
        }
    }
}