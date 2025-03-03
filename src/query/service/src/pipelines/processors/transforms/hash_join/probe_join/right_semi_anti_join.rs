// Copyright 2021 Datafuse Labs
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::sync::atomic::Ordering;

use common_exception::ErrorCode;
use common_exception::Result;
use common_expression::DataBlock;
use common_expression::Expr;
use common_expression::KeyAccessor;
use common_hashtable::HashJoinHashtableLike;
use common_hashtable::RowPtr;

use crate::pipelines::processors::transforms::hash_join::build_state::BuildBlockGenerationState;
use crate::pipelines::processors::transforms::hash_join::probe_state::ProbeBlockGenerationState;
use crate::pipelines::processors::transforms::hash_join::HashJoinProbeState;
use crate::pipelines::processors::transforms::hash_join::ProbeState;

impl HashJoinProbeState {
    pub(crate) fn right_semi_anti_join<'a, H: HashJoinHashtableLike>(
        &self,
        input: &DataBlock,
        keys: Box<(dyn KeyAccessor<Key = H::Key>)>,
        hash_table: &H,
        probe_state: &mut ProbeState,
    ) -> Result<Vec<DataBlock>>
    where
        H::Key: 'a,
    {
        // Probe states.
        let max_block_size = probe_state.max_block_size;
        let mutable_indexes = &mut probe_state.mutable_indexes;
        let build_indexes = &mut mutable_indexes.build_indexes;
        let build_indexes_ptr = build_indexes.as_mut_ptr();
        let pointers = probe_state.hashes.as_slice();

        // Build states.
        let build_state = unsafe { &mut *self.hash_join_state.build_state.get() };
        let outer_scan_map = &mut build_state.outer_scan_map;

        // Results.
        let mut matched_idx = 0;

        // Probe hash table and update `outer_scan_map`.
        if probe_state.probe_with_selection {
            let selection = &probe_state.selection.as_slice()[0..probe_state.selection_count];
            for idx in selection.iter() {
                let key = unsafe { keys.key_unchecked(*idx as usize) };
                let ptr = unsafe { *pointers.get_unchecked(*idx as usize) };

                // Probe hash table and fill `build_indexes`.
                let (mut match_count, mut incomplete_ptr) =
                    hash_table.next_probe(key, ptr, build_indexes_ptr, matched_idx, max_block_size);
                if match_count == 0 {
                    continue;
                }

                matched_idx += match_count;

                while matched_idx == max_block_size {
                    if self.hash_join_state.interrupt.load(Ordering::Relaxed) {
                        return Err(ErrorCode::AbortedQuery(
                            "Aborted query, because the server is shutting down or the query was killed.",
                        ));
                    }

                    for row_ptr in build_indexes.iter() {
                        unsafe {
                            *outer_scan_map
                                .get_unchecked_mut(row_ptr.chunk_index as usize)
                                .get_unchecked_mut(row_ptr.row_index as usize) = true
                        };
                    }

                    matched_idx = 0;
                    (match_count, incomplete_ptr) = hash_table.next_probe(
                        key,
                        incomplete_ptr,
                        build_indexes_ptr,
                        matched_idx,
                        max_block_size,
                    );
                    matched_idx += match_count;
                }
            }
        } else {
            for idx in 0..input.num_rows() {
                let key = unsafe { keys.key_unchecked(idx) };
                let ptr = unsafe { *pointers.get_unchecked(idx) };

                // Probe hash table and fill `build_indexes`.
                let (mut match_count, mut incomplete_ptr) =
                    hash_table.next_probe(key, ptr, build_indexes_ptr, matched_idx, max_block_size);
                if match_count == 0 {
                    continue;
                }

                matched_idx += match_count;
                while matched_idx == max_block_size {
                    if self.hash_join_state.interrupt.load(Ordering::Relaxed) {
                        return Err(ErrorCode::AbortedQuery(
                            "Aborted query, because the server is shutting down or the query was killed.",
                        ));
                    }

                    for row_ptr in build_indexes.iter() {
                        unsafe {
                            *outer_scan_map
                                .get_unchecked_mut(row_ptr.chunk_index as usize)
                                .get_unchecked_mut(row_ptr.row_index as usize) = true
                        };
                    }

                    matched_idx = 0;
                    (match_count, incomplete_ptr) = hash_table.next_probe(
                        key,
                        incomplete_ptr,
                        build_indexes_ptr,
                        matched_idx,
                        max_block_size,
                    );
                    matched_idx += match_count;
                }
            }
        }

        for row_ptr in build_indexes[0..matched_idx].iter() {
            unsafe {
                *outer_scan_map
                    .get_unchecked_mut(row_ptr.chunk_index as usize)
                    .get_unchecked_mut(row_ptr.row_index as usize) = true
            };
        }

        Ok(vec![])
    }

    pub(crate) fn right_semi_anti_join_with_conjunct<'a, H: HashJoinHashtableLike>(
        &self,
        input: &DataBlock,
        keys: Box<(dyn KeyAccessor<Key = H::Key>)>,
        hash_table: &H,
        probe_state: &mut ProbeState,
    ) -> Result<Vec<DataBlock>>
    where
        H::Key: 'a,
    {
        // Probe states.
        let max_block_size = probe_state.max_block_size;
        let mutable_indexes = &mut probe_state.mutable_indexes;
        let probe_indexes = &mut mutable_indexes.probe_indexes;
        let build_indexes = &mut mutable_indexes.build_indexes;
        let build_indexes_ptr = build_indexes.as_mut_ptr();
        let pointers = probe_state.hashes.as_slice();

        // Build states.
        let build_state = unsafe { &mut *self.hash_join_state.build_state.get() };
        let outer_scan_map = &mut build_state.outer_scan_map;
        let other_predicate = self
            .hash_join_state
            .hash_join_desc
            .other_predicate
            .as_ref()
            .unwrap();

        // Results.
        let mut matched_idx = 0;

        // Probe hash table and update `outer_scan_map`.
        if probe_state.probe_with_selection {
            let selection = &probe_state.selection.as_slice()[0..probe_state.selection_count];
            for idx in selection.iter() {
                let key = unsafe { keys.key_unchecked(*idx as usize) };
                let ptr = unsafe { *pointers.get_unchecked(*idx as usize) };

                // Probe hash table and fill `build_indexes`.
                let (mut match_count, mut incomplete_ptr) =
                    hash_table.next_probe(key, ptr, build_indexes_ptr, matched_idx, max_block_size);
                if match_count == 0 {
                    continue;
                }

                // Fill `probe_indexes`.
                for _ in 0..match_count {
                    unsafe { *probe_indexes.get_unchecked_mut(matched_idx) = *idx };
                    matched_idx += 1;
                }

                while matched_idx == max_block_size {
                    self.process_right_semi_anti_join_block(
                        matched_idx,
                        input,
                        probe_indexes,
                        build_indexes,
                        &mut probe_state.generation_state,
                        &build_state.generation_state,
                        outer_scan_map,
                        other_predicate,
                    )?;
                    matched_idx = 0;
                    (match_count, incomplete_ptr) = hash_table.next_probe(
                        key,
                        incomplete_ptr,
                        build_indexes_ptr,
                        matched_idx,
                        max_block_size,
                    );
                    for _ in 0..match_count {
                        unsafe { *probe_indexes.get_unchecked_mut(matched_idx) = *idx };
                        matched_idx += 1;
                    }
                }
            }
        } else {
            for idx in 0..input.num_rows() {
                let key = unsafe { keys.key_unchecked(idx) };
                let ptr = unsafe { *pointers.get_unchecked(idx) };
                let (mut match_count, mut incomplete_ptr) =
                    hash_table.next_probe(key, ptr, build_indexes_ptr, matched_idx, max_block_size);
                if match_count == 0 {
                    continue;
                }

                for _ in 0..match_count {
                    unsafe { *probe_indexes.get_unchecked_mut(matched_idx) = idx as u32 };
                    matched_idx += 1;
                }

                while matched_idx == max_block_size {
                    self.process_right_semi_anti_join_block(
                        matched_idx,
                        input,
                        probe_indexes,
                        build_indexes,
                        &mut probe_state.generation_state,
                        &build_state.generation_state,
                        outer_scan_map,
                        other_predicate,
                    )?;
                    matched_idx = 0;
                    (match_count, incomplete_ptr) = hash_table.next_probe(
                        key,
                        incomplete_ptr,
                        build_indexes_ptr,
                        matched_idx,
                        max_block_size,
                    );
                    for _ in 0..match_count {
                        unsafe { *probe_indexes.get_unchecked_mut(matched_idx) = idx as u32 };
                        matched_idx += 1;
                    }
                }
            }
        }

        if matched_idx > 0 {
            self.process_right_semi_anti_join_block(
                matched_idx,
                input,
                probe_indexes,
                build_indexes,
                &mut probe_state.generation_state,
                &build_state.generation_state,
                outer_scan_map,
                other_predicate,
            )?;
        }

        Ok(vec![])
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn process_right_semi_anti_join_block(
        &self,
        matched_idx: usize,
        input: &DataBlock,
        probe_indexes: &[u32],
        build_indexes: &[RowPtr],
        probe_state: &mut ProbeBlockGenerationState,
        build_state: &BuildBlockGenerationState,
        outer_scan_map: &mut [Vec<bool>],
        other_predicate: &Expr,
    ) -> Result<()> {
        if self.hash_join_state.interrupt.load(Ordering::Relaxed) {
            return Err(ErrorCode::AbortedQuery(
                "Aborted query, because the server is shutting down or the query was killed.",
            ));
        }

        let probe_block = if probe_state.is_probe_projected {
            Some(DataBlock::take(
                input,
                &probe_indexes[0..matched_idx],
                &mut probe_state.string_items_buf,
            )?)
        } else {
            None
        };
        let build_block = if build_state.is_build_projected {
            Some(self.hash_join_state.row_space.gather(
                &build_indexes[0..matched_idx],
                &build_state.build_columns,
                &build_state.build_columns_data_type,
                &build_state.build_num_rows,
                &mut probe_state.string_items_buf,
            )?)
        } else {
            None
        };

        let result_block = self.merge_eq_block(probe_block, build_block, matched_idx);

        if !result_block.is_empty() {
            let (bm, all_true, all_false) =
                self.get_other_filters(&result_block, other_predicate, &self.func_ctx)?;

            if all_true {
                for row_ptr in &build_indexes[0..matched_idx] {
                    unsafe {
                        *outer_scan_map
                            .get_unchecked_mut(row_ptr.chunk_index as usize)
                            .get_unchecked_mut(row_ptr.row_index as usize) = true
                    };
                }
            } else if !all_false {
                // Safe to unwrap.
                let validity = bm.unwrap();
                let mut idx = 0;
                while idx < matched_idx {
                    unsafe {
                        let valid = validity.get_bit_unchecked(idx);
                        if valid {
                            let row_ptr = build_indexes.get_unchecked(idx);
                            *outer_scan_map
                                .get_unchecked_mut(row_ptr.chunk_index as usize)
                                .get_unchecked_mut(row_ptr.row_index as usize) = true;
                        }
                    }
                    idx += 1;
                }
            }
        }
        Ok(())
    }
}
