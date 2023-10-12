/*
 * SPDX-FileCopyrightText: 2023 Inria
 * SPDX-FileCopyrightText: 2023 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

use crate::prelude::*;
use crate::traits::bit_field_slice::*;
use anyhow::Result;
use common_traits::*;
use core::marker::PhantomData;
use epserde::*;
use std::sync::atomic::*;
/**

A vector of values of fixed bit width.

Elements are stored contiguously, with no padding bits (in particular,
unless the bit width is a power of two some elements will be stored
across word boundaries).

We provide implementations
based on `AsRef<[usize]>`, `AsMut<[usize]>`, and
`AsRef<[AtomicUsize]>`. They implement
[`BitFieldSlice`], [`BitFieldSliceMut`], and [`BitFieldSliceAtomic`], respectively. Constructors are provided
for storing data in a [`Vec<usize>`](BitFieldVec::new) (for the first
two implementations) or in a
[`Vec<AtomicUsize>`](BitFieldVec::new_atomic) (for the third implementation).

In the latter case we can provide some concurrency guarantees,
albeit not full-fledged thread safety: more precisely, we can
guarantee thread-safety if the bit width is a power of two; otherwise,
concurrent writes to values that cross word boundaries might end
up in different threads succeding in writing only part of a value.
If the user can guarantee that no two threads ever write to the same
boundary-crossing value, then no race condition can happen.

*/
#[derive(Epserde, Debug, Clone, PartialEq, Eq, Hash)]
pub struct BitFieldVec<W = usize, M = W, B = Vec<W>> {
    /// The underlying storage.
    data: B,
    /// The bit width of the values stored in the array.
    bit_width: usize,
    /// A mask with its lowest `bit_width` bits set to one.
    mask: M,
    /// The length of the array.
    len: usize,

    _marker: PhantomData<W>,
}

fn mask<M: Word>(bit_width: usize) -> M {
    if bit_width == 0 {
        M::ZERO
    } else {
        M::MAX >> (M::BITS - bit_width)
    }
}

impl<W: Word> BitFieldVec<W, W, Vec<W>> {
    pub fn new(bit_width: usize, len: usize) -> Self {
        // We need at least one word to handle the case of bit width zero.
        let n_of_words = Ord::max(1, (len * bit_width + W::BITS - 1) / W::BITS);
        Self {
            data: vec![W::ZERO; n_of_words],
            bit_width,
            mask: mask(bit_width),
            len,
            _marker: PhantomData,
        }
    }
}

impl<W: Word + IntoAtomic> BitFieldVec<W, W, Vec<W>> {
    pub fn new_atomic(bit_width: usize, len: usize) -> BitFieldVec<W::AtomicType, W> {
        // we need at least two words to avoid branches in the gets
        let n_of_words = Ord::max(1, (len * bit_width + W::BITS - 1) / W::BITS);
        BitFieldVec::<W::AtomicType, W> {
            data: (0..n_of_words)
                .map(|_| W::AtomicType::new(W::ZERO))
                .collect(),
            bit_width,
            mask: mask(bit_width),
            len,
            _marker: PhantomData,
        }
    }
}

impl<W, M: Word, B> BitFieldVec<W, M, B> {
    /// # Safety
    /// `len` * `bit_width` must be between 0 (included) the number of
    /// bits in `data` (included).
    #[inline(always)]
    pub unsafe fn from_raw_parts(data: B, bit_width: usize, len: usize) -> Self {
        Self {
            data,
            bit_width,
            mask: mask(bit_width),
            len,
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    pub fn into_raw_parts(self) -> (B, usize, usize) {
        (self.data, self.bit_width, self.len)
    }
}

impl<W: AsBytes, M, T> BitFieldSliceCore<W> for BitFieldVec<W, M, T> {
    #[inline(always)]
    fn bit_width(&self) -> usize {
        debug_assert!(self.bit_width <= W::BITS);
        self.bit_width
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.len
    }
}

impl<W: Word, B: AsRef<[W]>> BitFieldSlice<W> for BitFieldVec<W, W, B> {
    #[inline]
    unsafe fn get_unchecked(&self, index: usize) -> W {
        let pos = index * self.bit_width;
        let word_index = pos / W::BITS;
        let bit_index = pos % W::BITS;

        if bit_index + self.bit_width <= W::BITS {
            (*self.data.as_ref().get_unchecked(word_index) >> bit_index) & self.mask
        } else {
            (*self.data.as_ref().get_unchecked(word_index) >> bit_index
                | *self.data.as_ref().get_unchecked(word_index + 1) << (W::BITS - bit_index))
                & self.mask
        }
    }
}

pub struct BitFieldVecUncheckedIterator<'a, W, M, B> {
    array: &'a BitFieldVec<W, M, B>,
    word_index: usize,
    window: W,
    fill: usize,
}

impl<'a, W: Word, B: AsRef<[W]>> BitFieldVecUncheckedIterator<'a, W, W, B> {
    fn new(array: &'a BitFieldVec<W, W, B>, index: usize) -> Self {
        if index > array.len() {
            panic!("Start index out of bounds: {} > {}", index, array.len());
        }
        let bit_offset = index * array.bit_width;
        let word_index = bit_offset / usize::BITS as usize;
        let fill;
        let window = if index == array.len() {
            fill = 0;
            W::ZERO
        } else {
            let bit_index = bit_offset % usize::BITS as usize;
            fill = usize::BITS as usize - bit_index;
            unsafe {
                // SAFETY: index has been check at the start and it is within bounds
                *array.data.as_ref().get_unchecked(word_index) >> bit_index
            }
        };
        Self {
            array,
            word_index,
            window,
            fill,
        }
    }
}

impl<'a, W: Word, B: AsRef<[W]>> UncheckedValueIterator
    for BitFieldVecUncheckedIterator<'a, W, W, B>
{
    type Item = W;
    unsafe fn next_unchecked(&mut self) -> W {
        if self.fill >= self.array.bit_width {
            self.fill -= self.array.bit_width;
            let res = self.window & self.array.mask;
            self.window >>= self.array.bit_width;
            return res;
        }

        let res = self.window;
        self.word_index += 1;
        self.window = *self.array.data.as_ref().get_unchecked(self.word_index);
        let res = (res | (self.window << self.fill)) & self.array.mask;
        let used = self.array.bit_width - self.fill;
        self.window >>= used;
        self.fill = usize::BITS as usize - used;
        res
    }
}

impl<W: Word, B: AsRef<[W]>> IntoUncheckedValueIterator for BitFieldVec<W, W, B> {
    type Item = W;
    type IntoUncheckedValueIter<'a> = BitFieldVecUncheckedIterator<'a, W, W, B>
        where B:'a, W:'a ;

    fn iter_val_from_unchecked(&self, from: usize) -> Self::IntoUncheckedValueIter<'_> {
        BitFieldVecUncheckedIterator::new(self, from)
    }
}

pub struct BitFieldVecIterator<'a, W, M, B> {
    unchecked: BitFieldVecUncheckedIterator<'a, W, M, B>,
    index: usize,
}

impl<'a, W: Word, B: AsRef<[W]>> BitFieldVecIterator<'a, W, W, B> {
    fn new(array: &'a BitFieldVec<W, W, B>, index: usize) -> Self {
        if index > array.len() {
            panic!("Start index out of bounds: {} > {}", index, array.len());
        }
        Self {
            unchecked: BitFieldVecUncheckedIterator::new(array, index),
            index,
        }
    }
}

impl<'a, W: Word, B: AsRef<[W]>> Iterator for BitFieldVecIterator<'a, W, W, B> {
    type Item = W;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.unchecked.array.len() {
            // SAFETY: index has just been checked.
            let res = unsafe { self.unchecked.next_unchecked() };
            self.index += 1;
            Some(res)
        } else {
            None
        }
    }
}

impl<'a, W: Word, B: AsRef<[W]>> ExactSizeIterator for BitFieldVecIterator<'a, W, W, B> {
    fn len(&self) -> usize {
        self.unchecked.array.len() - self.index
    }
}

impl<W: Word, B: AsRef<[W]>> IntoValueIterator for BitFieldVec<W, W, B> {
    type Item = W;
    type IntoValueIter<'a> = BitFieldVecIterator<'a, W, W, B>
        where B:'a, W: 'a;

    fn iter_val_from(&self, from: usize) -> Self::IntoValueIter<'_> {
        BitFieldVecIterator::new(self, from)
    }
}

impl<W: Word, B: AsRef<[W]>> BitFieldVec<W, W, B> {
    /// Convenience method that delegates to [`IntoValueIterator::iter_val`]
    /// and returns an [`ExactSizeIterator`].
    #[inline(always)]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = W> + '_ {
        self.iter_val()
    }

    /// Convenience method that delegates to [`IntoValueIterator::iter_val_from`]
    /// and returns an [`ExactSizeIterator`].
    #[inline(always)]
    pub fn iter_from(&self, from: usize) -> impl ExactSizeIterator<Item = W> + '_ {
        self.iter_val_from(from)
    }
}

impl<W: Word, B: AsRef<[W]> + AsMut<[W]>> BitFieldSliceMut<W> for BitFieldVec<W, W, B> {
    // We reimplement set as we have the mask in the structure.

    /// Set the element of the slice at the specified index.
    ///
    ///
    /// May panic if the index is not in in [0..[len](`BitFieldSliceCore::len`))
    /// or the value does not fit in [`BitFieldSliceCore::bit_width`] bits.
    #[inline(always)]
    fn set(&mut self, index: usize, value: W) {
        panic_if_out_of_bounds!(index, self.len);
        panic_if_value!(value, self.mask, self.bit_width);
        unsafe {
            self.set_unchecked(index, value);
        }
    }

    #[inline]
    unsafe fn set_unchecked(&mut self, index: usize, value: W) {
        let pos = index * self.bit_width;
        let word_index = pos / W::BITS;
        let bit_index = pos % W::BITS;

        if bit_index + self.bit_width <= W::BITS {
            let mut word = *self.data.as_ref().get_unchecked(word_index);
            word &= !(self.mask << bit_index);
            word |= value << bit_index;
            *self.data.as_mut().get_unchecked_mut(word_index) = word;
        } else {
            let mut word = *self.data.as_ref().get_unchecked(word_index);
            word &= (W::ONE << bit_index) - W::ONE;
            word |= value << bit_index;
            *self.data.as_mut().get_unchecked_mut(word_index) = word;

            let mut word = *self.data.as_ref().get_unchecked(word_index + 1);
            word &= !(self.mask >> (W::BITS - bit_index));
            word |= value >> (W::BITS - bit_index);
            *self.data.as_mut().get_unchecked_mut(word_index + 1) = word;
        }
    }
}

impl<W: Word + IntoAtomic, T: AsRef<[W::AtomicType]>> BitFieldSliceAtomic<W>
    for BitFieldVec<W::AtomicType, W, T>
where
    W::AtomicType: AtomicUnsignedInt + AsBytes,
{
    #[inline]
    unsafe fn get_unchecked(&self, index: usize, order: Ordering) -> W {
        let pos = index * self.bit_width;
        let word_index = pos / W::BITS;
        let bit_index = pos % W::BITS;
        let data: &[W::AtomicType] = self.data.as_ref();

        if bit_index + self.bit_width <= W::BITS {
            (data.get_unchecked(word_index).load(order) >> bit_index) & self.mask
        } else {
            (data.get_unchecked(word_index).load(order) >> bit_index
                | data.get_unchecked(word_index + 1).load(order) << (W::BITS - bit_index))
                & self.mask
        }
    }

    // We reimplement set as we have the mask in the structure.

    /// Set the element of the slice at the specified index.
    ///
    ///
    /// May panic if the index is not in in [0..[len](`BitFieldSliceCore::len`))
    /// or the value does not fit in [`BitFieldSliceCore::bit_width`] bits.
    #[inline(always)]
    fn set(&self, index: usize, value: W, order: Ordering) {
        panic_if_out_of_bounds!(index, self.len);
        panic_if_value!(value, self.mask, self.bit_width);
        unsafe {
            self.set_unchecked(index, value, order);
        }
    }

    #[inline]
    unsafe fn set_unchecked(&self, index: usize, value: W, order: Ordering) {
        debug_assert!(self.bit_width != W::BITS);
        let pos = index * self.bit_width;
        let word_index = pos / W::BITS;
        let bit_index = pos % W::BITS;
        let data: &[W::AtomicType] = self.data.as_ref();

        if bit_index + self.bit_width <= W::BITS {
            // this is consistent
            let mut current = data.get_unchecked(word_index).load(order);
            loop {
                let mut new = current;
                new &= !(self.mask << bit_index);
                new |= value << bit_index;

                match data
                    .get_unchecked(word_index)
                    .compare_exchange(current, new, order, order)
                {
                    Ok(_) => break,
                    Err(e) => current = e,
                }
            }
        } else {
            let mut word = data.get_unchecked(word_index).load(order);
            // try to wait for the other thread to finish
            fence(Ordering::Acquire);
            loop {
                let mut new = word;
                new &= (W::ONE << bit_index) - W::ONE;
                new |= value << bit_index;

                match data
                    .get_unchecked(word_index)
                    .compare_exchange(word, new, order, order)
                {
                    Ok(_) => break,
                    Err(e) => word = e,
                }
            }
            fence(Ordering::Release);

            // ensure that the compiler does not reorder the two atomic operations
            // this should increase the probability of having consistency
            // between two concurrent writes as they will both execute the set
            // of the bits in the same order, and the release / acquire fence
            // should try to syncronize the threads as much as possible
            compiler_fence(Ordering::SeqCst);

            let mut word = data.get_unchecked(word_index + 1).load(order);
            fence(Ordering::Acquire);
            loop {
                let mut new = word;
                new &= !(self.mask >> (W::BITS - bit_index));
                new |= value >> (W::BITS - bit_index);

                match data
                    .get_unchecked(word_index + 1)
                    .compare_exchange(word, new, order, order)
                {
                    Ok(_) => break,
                    Err(e) => word = e,
                }
            }
            fence(Ordering::Release);
        }
    }
}

/// Provide conversion betweeen compact arrays whose backends
/// are [convertible](ConvertTo) into one another.
///
/// Many implementations of this trait are then used to
/// implement by delegation a corresponding [`From`].
impl<V, W, M, B, C> ConvertTo<BitFieldVec<W, M, C>> for BitFieldVec<V, M, B>
where
    B: ConvertTo<C>,
{
    #[inline]
    fn convert_to(self) -> Result<BitFieldVec<W, M, C>> {
        Ok(BitFieldVec {
            len: self.len,
            bit_width: self.bit_width,
            mask: self.mask,
            data: self.data.convert_to()?,
            _marker: PhantomData,
        })
    }
}

macro_rules! impl_from {
    ($std:ty, $atomic:ty) => {
        impl From<BitFieldVec<$std>> for BitFieldVec<$atomic, $std> {
            #[inline]
            fn from(bm: BitFieldVec<$std>) -> Self {
                bm.convert_to().unwrap()
            }
        }

        impl From<BitFieldVec<$atomic, $std>> for BitFieldVec<$std> {
            #[inline]
            fn from(bm: BitFieldVec<$atomic, $std>) -> Self {
                bm.convert_to().unwrap()
            }
        }

        impl<'a> From<BitFieldVec<$std, $std, &'a [$std]>>
            for BitFieldVec<$atomic, $std, &'a [$atomic]>
        {
            #[inline]
            fn from(bm: BitFieldVec<$std, $std, &'a [$std]>) -> Self {
                bm.convert_to().unwrap()
            }
        }

        impl<'a> From<BitFieldVec<$atomic, $std, &'a [$atomic]>>
            for BitFieldVec<$std, $std, &'a [$std]>
        {
            #[inline]
            fn from(bm: BitFieldVec<$atomic, $std, &'a [$atomic]>) -> Self {
                bm.convert_to().unwrap()
            }
        }
    };
}

impl_from!(u8, AtomicU8);
impl_from!(u16, AtomicU16);
impl_from!(u32, AtomicU32);
impl_from!(u64, AtomicU64);
impl_from!(usize, AtomicUsize);
