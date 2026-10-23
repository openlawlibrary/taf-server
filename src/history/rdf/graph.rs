#![expect(
    clippy::module_name_repetitions,
    reason = "Call our graph `FondsGraph`, which repeats the name graph, but is used to differentiate between our wrapper and the underlying sophia graph."
)]
#![expect(
    clippy::min_ident_chars,
    reason = "Nomenclature for RDF is `s`, `p, `o` instead of `subject`, `predicate`, `object`"
)]
#![expect(
    clippy::pattern_type_mismatch,
    reason = "Bypass sophia internal & ref match on SimpleTerm"
)]
/// The helper methods for working with RDF in Fonds.
use anyhow::Context as _;
use sophia::api::graph::{GTripleSource, Graph as _};
use sophia::api::ns::NsTerm;
use sophia::api::MownStr;
use sophia::api::{prelude::*, term::SimpleTerm};
use sophia::inmem::graph::FastGraph;
use std::iter;
/// Fonds representation of an RDF graph.
pub struct FondsGraph {
    /// The underlying `sophia` graph.
    pub fast_graph: FastGraph,
}

impl Default for FondsGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl FondsGraph {
    /// Create a new graph.
    #[must_use]
    pub fn new() -> Self {
        Self {
            fast_graph: FastGraph::new(),
        }
    }
    /// Extract a literal from a triple matching.
    ///
    /// # Errors
    /// Errors if the triple matching the object is not found.
    /// Errors if the object is not an RDF literal.
    pub fn literal_from_triple_matching(
        &self,
        subject: Option<&SimpleTerm>,
        predicate: Option<NsTerm>,
        object: Option<NsTerm>,
    ) -> anyhow::Result<String> {
        let triple = self.get_next_triples_matching(subject, predicate, object)?;
        let literal = self.term_to_literal(&triple)?;
        Ok(literal)
    }

    /// Convert a term to a literal.
    ///
    /// # Errors
    /// Errors if the term is not an RDF literal.
    pub fn term_to_literal(&self, term: &[&SimpleTerm<'_>; 3]) -> anyhow::Result<String> {
        match &term.o() {
            SimpleTerm::LiteralDatatype(literal, _) | SimpleTerm::LiteralLanguage(literal, _, _) => {
                Ok(literal.to_string())
            }
            SimpleTerm::Iri(_)
            | SimpleTerm::BlankNode(_)
            | SimpleTerm::Triple(_)
            | SimpleTerm::Variable(_) => {
                anyhow::bail!("Expected literal language, got - {term:?}")
            }
        }
    }

    /// Extract all literals from a triple matching.
    ///
    /// # Errors
    /// Errors if the triple matching the object is not found.
    /// Errors if the object is not an RDF literal.
    pub fn all_literals_from_triple_matching(
        &self,
        subject: Option<&SimpleTerm>,
        predicate: Option<NsTerm>,
        object: Option<NsTerm>,
    ) -> anyhow::Result<Vec<String>> {
        let mut literals = Vec::new();
        let triples_iter = self.triples_matching_inner(subject, predicate, object);
        for term in triples_iter {
            literals.push(self.term_to_literal(&term?)?);
        }
        Ok(literals)
    }

    /// Extract an IRI from a triple matching.
    ///
    /// # Errors
    /// Errors if the triple matching the object is not found.
    /// Errors if the object is not an RDF IRI.
    pub fn iri_from_triple_matching<'graph>(
        &'graph self,
        subject: Option<&'graph SimpleTerm>,
        predicate: Option<NsTerm<'graph>>,
        object: Option<NsTerm<'graph>>,
    ) -> anyhow::Result<SimpleTerm<'graph>> {
        let triple = self.get_next_triples_matching(subject, predicate, object)?;
        let SimpleTerm::Iri(iri) = &triple.o() else {
            anyhow::bail!("Expected literal language, got - {:?}", triple.o());
        };
        Ok(SimpleTerm::Iri(iri.clone()))
    }

    /// Returns the next triple matching the given subject, predicate, and object.
    ///
    /// # Errors
    /// Errors if the triple matching the object is not found.
    fn get_next_triples_matching<'graph>(
        &'graph self,
        subject: Option<&'graph SimpleTerm>,
        predicate: Option<NsTerm<'graph>>,
        object: Option<NsTerm<'graph>>,
    ) -> anyhow::Result<[&'graph SimpleTerm<'graph>; 3]> {
        let triple = self
            .triples_matching_inner(subject, predicate, object)
            .next()
            .context(format!(
                "Expected to find triple matching s={subject:?}, p={predicate:?}, o={object:?}"
            ))?;
        Ok(triple?)
    }

    /// Utility method to convert from Option method arguments to a triple source.
    fn triples_matching_inner<'graph>(
        &'graph self,
        subject: Option<&'graph SimpleTerm>,
        predicate: Option<NsTerm<'graph>>,
        object: Option<NsTerm<'graph>>,
    ) -> GTripleSource<'graph, FastGraph> {
        let triple = match (subject, predicate, object) {
            (Some(s), None, None) => self.fast_graph.triples_matching([s], Any, Any),
            (None, Some(p), None) => self.fast_graph.triples_matching(Any, [p], Any),
            (None, None, Some(o)) => self.fast_graph.triples_matching(Any, Any, [o]),
            (Some(s), Some(p), None) => self.fast_graph.triples_matching([s], [p], Any),
            (Some(s), None, Some(o)) => self.fast_graph.triples_matching([s], Any, [o]),
            (None, Some(p), Some(o)) => self.fast_graph.triples_matching(Any, [p], [o]),
            (Some(s), Some(p), Some(o)) => self.fast_graph.triples_matching([s], [p], [o]),
            (None, None, None) => Box::new(iter::empty()),
        };
        triple
    }

    /// Extract all IRIs from a triple matching.
    ///
    /// # Errors
    /// Errors if the triple matching the object is not found.
    /// Errors if the object is not an RDF IRI.
    pub fn all_iris_from_triple_matching<'graph>(
        &'graph self,
        subject: Option<&'graph SimpleTerm>,
        predicate: Option<NsTerm<'graph>>,
        object: Option<NsTerm<'graph>>,
    ) -> anyhow::Result<Vec<&'graph SimpleTerm<'graph>>> {
        let triples_iter = self.triples_matching_inner(subject, predicate, object);
        let iris = triples_iter
            .into_iter()
            .filter_map(|triple| {
                let found_triple = triple.ok()?;
                let subj = found_triple.s();
                Some(subj)
            })
            .collect();
        Ok(iris)
    }
}

/// Unordered container of RDF items.
pub struct Bag<'graph> {
    /// The container URI.
    uri: SimpleTerm<'graph>,
    /// The underlying graph.
    graph: &'graph FondsGraph,
}

impl Bag<'_> {
    /// Create a new Bag.
    #[must_use]
    pub const fn new<'graph>(graph: &'graph FondsGraph, uri: SimpleTerm<'graph>) -> Bag<'graph> {
        Bag { uri, graph }
    }

    /// Extract items from the container.
    ///
    /// # Errors
    /// Errors if the items are not found.
    pub fn items(&self) -> anyhow::Result<Vec<SimpleTerm<'_>>> {
        let container = &self.uri;
        let mut i: u32 = 1;
        let mut items = vec![];
        loop {
            let el_uri = format!("http://www.w3.org/1999/02/22-rdf-syntax-ns#_{i}");
            let elem_iri = SimpleTerm::Iri(IriRef::new_unchecked(MownStr::from_str(&el_uri)));
            let item_response = self
                .graph
                .fast_graph
                .triples_matching([container], Some(elem_iri), Any)
                .next();
            if let Some(found_item) = item_response {
                i += 1;
                let item = found_item
                    .context(format!("Expected to find item in {container:?}"))?
                    .o()
                    .clone();
                items.push(item);
            } else {
                break;
            }
        }
        Ok(items)
    }
}
