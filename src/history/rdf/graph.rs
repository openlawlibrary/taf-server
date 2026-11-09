#![expect(
    clippy::module_name_repetitions,
    reason = "Call our graph `FondsGraph`, which repeats the name graph, but is used to differentiate between our wrapper and the underlying sophia graph."
)]
#![expect(
    clippy::min_ident_chars,
    reason = "Nomenclature for RDF is `s`, `p, `o` instead of `subject`, `predicate`, `object`"
)]
/// The helper methods for working with RDF in Fonds.
use anyhow::Context as _;
use sophia::api::graph::{GResult, GTerm, Graph as _};
use sophia::api::ns::NsTerm;
use sophia::api::prelude::*;
use sophia::inmem::graph::FastGraph;
use std::iter;

/// A term borrowed from a [`FondsGraph`].
pub type FondsTerm<'graph> = GTerm<'graph, FastGraph>;

/// A triple borrowed from a [`FondsGraph`].
pub type FondsTriple<'graph> = [FondsTerm<'graph>; 3];

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
    pub fn literal_from_triple_matching<'graph>(
        &'graph self,
        subject: Option<FondsTerm<'graph>>,
        predicate: Option<NsTerm<'graph>>,
        object: Option<NsTerm<'graph>>,
    ) -> anyhow::Result<String> {
        let triple = self.get_next_triples_matching(subject, predicate, object)?;
        let literal = self.term_to_literal(&triple)?;
        Ok(literal)
    }

    /// Convert a term to a literal.
    ///
    /// # Errors
    /// Errors if the term is not an RDF literal.
    pub fn term_to_literal(&self, term: &FondsTriple<'_>) -> anyhow::Result<String> {
        term.o()
            .lexical_form()
            .map(|literal| literal.to_string())
            .with_context(|| format!("Expected literal, got - {:?}", term.o().as_simple()))
    }

    /// Extract all literals from a triple matching.
    ///
    /// # Errors
    /// Errors if the triple matching the object is not found.
    /// Errors if the object is not an RDF literal.
    pub fn all_literals_from_triple_matching<'graph>(
        &'graph self,
        subject: Option<FondsTerm<'graph>>,
        predicate: Option<NsTerm<'graph>>,
        object: Option<NsTerm<'graph>>,
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
        subject: Option<FondsTerm<'graph>>,
        predicate: Option<NsTerm<'graph>>,
        object: Option<NsTerm<'graph>>,
    ) -> anyhow::Result<FondsTerm<'graph>> {
        let triple = self.get_next_triples_matching(subject, predicate, object)?;
        let iri = triple.o();
        if !iri.is_iri() {
            anyhow::bail!("Expected IRI, got - {:?}", iri.as_simple());
        }
        Ok(iri)
    }

    /// Returns the next triple matching the given subject, predicate, and object.
    ///
    /// # Errors
    /// Errors if the triple matching the object is not found.
    fn get_next_triples_matching<'graph>(
        &'graph self,
        subject: Option<FondsTerm<'graph>>,
        predicate: Option<NsTerm<'graph>>,
        object: Option<NsTerm<'graph>>,
    ) -> anyhow::Result<FondsTriple<'graph>> {
        let triple = self
            .triples_matching_inner(subject, predicate, object)
            .next()
            .with_context(|| {
                format!(
                    "Expected to find triple matching s={:?}, p={predicate:?}, o={object:?}",
                    subject.as_ref().map(Term::as_simple)
                )
            })?;
        Ok(triple?)
    }

    /// Utility method to convert from Option method arguments to a triple source.
    fn triples_matching_inner<'graph>(
        &'graph self,
        subject: Option<FondsTerm<'graph>>,
        predicate: Option<NsTerm<'graph>>,
        object: Option<NsTerm<'graph>>,
    ) -> Box<dyn Iterator<Item = GResult<FastGraph, FondsTriple<'graph>>> + 'graph> {
        let graph = &self.fast_graph;
        match (subject, predicate, object) {
            (Some(s), None, None) => Box::new(graph.triples_matching([s], Any, Any)),
            (None, Some(p), None) => Box::new(graph.triples_matching(Any, [p], Any)),
            (None, None, Some(o)) => Box::new(graph.triples_matching(Any, Any, [o])),
            (Some(s), Some(p), None) => Box::new(graph.triples_matching([s], [p], Any)),
            (Some(s), None, Some(o)) => Box::new(graph.triples_matching([s], Any, [o])),
            (None, Some(p), Some(o)) => Box::new(graph.triples_matching(Any, [p], [o])),
            (Some(s), Some(p), Some(o)) => Box::new(graph.triples_matching([s], [p], [o])),
            (None, None, None) => Box::new(iter::empty()),
        }
    }

    /// Extract all IRIs from a triple matching.
    ///
    /// # Errors
    /// Errors if the triple matching the object is not found.
    /// Errors if the object is not an RDF IRI.
    pub fn all_iris_from_triple_matching<'graph>(
        &'graph self,
        subject: Option<FondsTerm<'graph>>,
        predicate: Option<NsTerm<'graph>>,
        object: Option<NsTerm<'graph>>,
    ) -> anyhow::Result<Vec<FondsTerm<'graph>>> {
        let triples_iter = self.triples_matching_inner(subject, predicate, object);
        let iris = triples_iter
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
    uri: FondsTerm<'graph>,
    /// The underlying graph.
    graph: &'graph FondsGraph,
}

impl<'graph> Bag<'graph> {
    /// Create a new Bag.
    #[must_use]
    pub const fn new(graph: &'graph FondsGraph, uri: FondsTerm<'graph>) -> Self {
        Bag { uri, graph }
    }

    /// Extract items from the container.
    ///
    /// # Errors
    /// Errors if the items are not found.
    pub fn items(&self) -> anyhow::Result<Vec<FondsTerm<'graph>>> {
        let container = self.uri;
        let mut i: u32 = 1;
        let mut items = vec![];
        loop {
            let el_uri = format!("http://www.w3.org/1999/02/22-rdf-syntax-ns#_{i}");
            let elem_iri = IriRef::new_unchecked(el_uri.as_str());
            let item_response = self
                .graph
                .fast_graph
                .triples_matching([container], [elem_iri], Any)
                .next();
            if let Some(found_item) = item_response {
                i += 1;
                let item = found_item
                    .with_context(|| {
                        format!("Expected to find item in {:?}", container.as_simple())
                    })?
                    .o();
                items.push(item);
            } else {
                break;
            }
        }
        Ok(items)
    }
}
