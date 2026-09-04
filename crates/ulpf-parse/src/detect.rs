//! Signature detection. Cheap substring finders first; a per-source hint lets the
//! engine try the last successful parser before the ordered scan.

use memchr::memmem::Finder;
use regex::bytes::Regex;

use crate::Scratch;
use crate::compile::Parser;
use crate::def::Matcher;

pub(crate) struct CompiledMatcher {
    finders: Vec<Finder<'static>>,
    starts_with: Option<Vec<u8>>,
    regex: Option<Regex>,
}

impl CompiledMatcher {
    pub(crate) fn compile(m: &Matcher) -> Result<Self, String> {
        if m.contains.is_empty() && m.starts_with.is_none() && m.regex.is_none() {
            return Err("[match] needs at least one of `contains`, `starts_with`, `regex`".into());
        }
        let regex = match &m.regex {
            Some(r) => Some(Regex::new(&format!("(?s-u){r}")).map_err(|e| format!("[match] regex: {e}"))?),
            None => None,
        };
        Ok(CompiledMatcher {
            finders: m.contains.iter().map(|s| Finder::new(s.as_bytes()).into_owned()).collect(),
            starts_with: m.starts_with.as_ref().map(|s| s.as_bytes().to_vec()),
            regex,
        })
    }

    #[inline]
    pub(crate) fn matches(&self, event: &[u8]) -> bool {
        if let Some(p) = &self.starts_with
            && !event.starts_with(p)
        {
            return false;
        }
        if !self.finders.iter().all(|f| f.find(event).is_some()) {
            return false;
        }
        self.regex.as_ref().is_none_or(|r| r.is_match(event))
    }
}

pub struct Registry {
    parsers: Vec<Parser>,
    /// Indices into `parsers`, highest priority first, then by name.
    order: Vec<usize>,
}

impl Registry {
    pub fn new(parsers: Vec<Parser>) -> Registry {
        let mut order: Vec<usize> = (0..parsers.len()).collect();
        order.sort_by(|&a, &b| {
            parsers[b].definition().matcher.priority
                .cmp(&parsers[a].definition().matcher.priority)
                .then_with(|| parsers[a].name().cmp(parsers[b].name()))
        });
        Registry { parsers, order }
    }

    pub fn len(&self) -> usize {
        self.parsers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parsers.is_empty()
    }

    pub fn get(&self, idx: usize) -> &Parser {
        &self.parsers[idx]
    }

    pub fn iter(&self) -> impl Iterator<Item = &Parser> {
        self.parsers.iter()
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.parsers.iter().position(|p| p.name() == name)
    }

    pub fn scratch(&self) -> Scratch {
        Scratch::default()
    }

    /// Index of the first parser whose signature matches, trying `hint` first.
    pub fn detect(&self, event: &[u8], hint: Option<usize>) -> Option<usize> {
        if let Some(h) = hint
            && self.parsers.get(h).is_some_and(|p| p.matches(event))
        {
            return Some(h);
        }
        self.order.iter().copied().find(|&i| self.parsers[i].matches(event))
    }
}
