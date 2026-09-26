//! Pass 1 DOI clustering, pass 2 blocked fuzzy matching and group assembly.

use super::{BlockTask, DedupRecord, Deduplicator, DuplicateGroup, UnionFind};
use crate::Citation;
use std::collections::{BTreeMap, HashMap};

impl Deduplicator {
    pub(super) fn run_pass1_doi_clustering(
        &self,
        records: &[DedupRecord],
        union_find: &mut UnionFind,
    ) {
        let mut doi_buckets: HashMap<String, Vec<usize>> = HashMap::new();
        for record in records {
            if let Some(norm_doi) = &record.norm_doi {
                doi_buckets
                    .entry(norm_doi.clone())
                    .or_default()
                    .push(record.idx);
            }
        }

        // TODO: extend to PMID / accession_number keys.
        for bucket in doi_buckets.values() {
            for left_idx in 0..bucket.len() {
                for right_idx in (left_idx + 1)..bucket.len() {
                    let left = &records[bucket[left_idx]];
                    let right = &records[bucket[right_idx]];

                    let should_union = if left.norm_title.is_empty() || right.norm_title.is_empty()
                    {
                        Self::has_metadata_agreement(left, right)
                    } else {
                        self.pass1_same_doi_titles_match(left, right)
                    };

                    if should_union {
                        union_find.union(left.idx, right.idx);
                    }
                }
            }
        }
    }

    pub(super) fn collect_pass2_matches(
        &self,
        records: &[DedupRecord],
        pass1_roots: &[usize],
    ) -> Vec<(usize, usize)> {
        let tasks = self.build_pass2_block_tasks(records);
        let mut matches = if self.parallel {
            use rayon::prelude::*;

            tasks
                .par_iter()
                .flat_map_iter(|task| self.evaluate_block_task(task, records, pass1_roots))
                .collect::<Vec<_>>()
        } else {
            let mut matches = Vec::new();
            for task in &tasks {
                matches.extend(self.evaluate_block_task(task, records, pass1_roots));
            }
            matches
        };

        matches.sort_unstable();
        matches.dedup();
        matches
    }

    pub(super) fn build_pass2_block_tasks(&self, records: &[DedupRecord]) -> Vec<BlockTask> {
        let mut year_blocks: BTreeMap<i32, Vec<usize>> = BTreeMap::new();
        let mut unknown_year_block = Vec::new();

        for record in records
            .iter()
            .filter(|record| !record.norm_title.is_empty())
        {
            match record.year {
                Some(year) => {
                    let max_year = year.saturating_add(i32::from(self.year_tolerance));
                    for block_year in year..=max_year {
                        year_blocks.entry(block_year).or_default().push(record.idx);
                    }
                }
                None => unknown_year_block.push(record.idx),
            }
        }

        let mut tasks = Vec::new();
        for members in year_blocks.values_mut() {
            members.sort_unstable();
            members.dedup();
            if members.len() > 1 {
                // TODO: sorted-token fingerprint sub-blocking for very large year blocks.
                // The unknown-year block is compared against every year block: O(u x n) if a
                // source systematically lacks dates.
                tasks.push(BlockTask::Within(members.clone()));
            }
        }

        unknown_year_block.sort_unstable();
        unknown_year_block.dedup();
        if unknown_year_block.len() > 1 {
            tasks.push(BlockTask::Within(unknown_year_block.clone()));
        }

        if !unknown_year_block.is_empty() {
            for members in year_blocks.values() {
                if !members.is_empty() {
                    tasks.push(BlockTask::Cross(
                        unknown_year_block.clone(),
                        members.clone(),
                    ));
                }
            }
        }

        tasks
    }

    pub(super) fn evaluate_block_task(
        &self,
        task: &BlockTask,
        records: &[DedupRecord],
        pass1_roots: &[usize],
    ) -> Vec<(usize, usize)> {
        let mut matches = Vec::new();

        match task {
            BlockTask::Within(members) => {
                for left_pos in 0..members.len() {
                    for right_pos in (left_pos + 1)..members.len() {
                        let left = members[left_pos];
                        let right = members[right_pos];
                        if pass1_roots[left] == pass1_roots[right] {
                            continue;
                        }

                        if self.records_match(&records[left], &records[right]) {
                            matches.push((left, right));
                        }
                    }
                }
            }
            BlockTask::Cross(left_members, right_members) => {
                for &left in left_members {
                    for &right in right_members {
                        if left == right {
                            continue;
                        }

                        let pair = Self::ordered_pair(left, right);
                        if pass1_roots[pair.0] == pass1_roots[pair.1] {
                            continue;
                        }

                        if self.records_match(&records[pair.0], &records[pair.1]) {
                            matches.push(pair);
                        }
                    }
                }
            }
        }

        matches
    }

    pub(super) fn assemble_groups(
        &self,
        citations: &[Citation],
        sources: &[&str],
        union_find: &mut UnionFind,
    ) -> Vec<DuplicateGroup> {
        let mut components: HashMap<usize, Vec<usize>> = HashMap::new();
        for idx in 0..citations.len() {
            let root = union_find.find(idx);
            components.entry(root).or_default().push(idx);
        }

        components
            .into_values()
            .map(|mut members| {
                members.sort_unstable();
                let unique = self.select_unique_citation_index(citations, sources, &members);
                let duplicates = members
                    .into_iter()
                    .filter(|&idx| idx != unique)
                    .collect::<Vec<_>>();

                DuplicateGroup { unique, duplicates }
            })
            .collect()
    }

    pub(super) fn select_unique_citation_index(
        &self,
        citations: &[Citation],
        sources: &[&str],
        group_indices: &[usize],
    ) -> usize {
        if group_indices.len() == 1 {
            return group_indices[0];
        }

        for preferred_source in &self.source_preferences {
            for &idx in group_indices {
                if sources.get(idx).copied() == Some(preferred_source.as_str()) {
                    return idx;
                }
            }
        }

        let citations_with_abstract = group_indices
            .iter()
            .copied()
            .filter(|&idx| {
                citations[idx]
                    .abstract_text
                    .as_deref()
                    .is_some_and(|abstract_text| !abstract_text.trim().is_empty())
            })
            .collect::<Vec<_>>();

        if citations_with_abstract.is_empty() {
            return group_indices[0];
        }

        citations_with_abstract
            .iter()
            .copied()
            .find(|&idx| {
                citations[idx]
                    .doi
                    .as_deref()
                    .is_some_and(|doi| !doi.trim().is_empty())
            })
            .unwrap_or(citations_with_abstract[0])
    }

    pub(super) fn sort_duplicate_groups(duplicate_groups: &mut [DuplicateGroup]) {
        duplicate_groups.sort_by_key(|group| {
            group
                .duplicates
                .iter()
                .copied()
                .chain(std::iter::once(group.unique))
                .min()
                .unwrap_or(group.unique)
        });
    }
}

impl UnionFind {
    pub(super) fn new(len: usize) -> Self {
        Self {
            parents: (0..len).collect(),
            ranks: vec![0; len],
        }
    }

    pub(super) fn find(&mut self, idx: usize) -> usize {
        if self.parents[idx] != idx {
            let parent = self.parents[idx];
            self.parents[idx] = self.find(parent);
        }
        self.parents[idx]
    }

    pub(super) fn union(&mut self, left: usize, right: usize) {
        let left_root = self.find(left);
        let right_root = self.find(right);

        if left_root == right_root {
            return;
        }

        match self.ranks[left_root].cmp(&self.ranks[right_root]) {
            std::cmp::Ordering::Less => self.parents[left_root] = right_root,
            std::cmp::Ordering::Greater => self.parents[right_root] = left_root,
            std::cmp::Ordering::Equal => {
                self.parents[right_root] = left_root;
                self.ranks[left_root] += 1;
            }
        }
    }

    pub(super) fn snapshot_roots(&mut self) -> Vec<usize> {
        (0..self.parents.len()).map(|idx| self.find(idx)).collect()
    }
}
