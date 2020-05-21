use std::collections::VecDeque;
use std::ops::{Add, Div, Mul, Sub};

pub trait Identity {
    fn zero() -> Self;
    fn one() -> Self;
}

pub trait Dividable
where
    Self: Div<<Self as Dividable>::Divider, Output = Self> + Sized,
{
    type Divider;
}

macro_rules! impl_identity_and_dividable {
    ($Z:expr, $O:expr, $($T:ty),*) => (
        $(
            impl Identity for $T {
                fn zero() -> Self { $Z }
                fn one() -> Self { $O }
            }
            impl Dividable for $T {
                type Divider = $T;
            }
        )*
    )
}

impl_identity_and_dividable!(0, 1, u8, u16, u32, u64, u128, i8, i16, i32, i64, i128);
impl_identity_and_dividable!(0., 1., f32, f64);

pub trait Indicator<E>
where
    Self: Sized,
{
    type Output;
    fn new(size: usize) -> Result<Self, &'static str>;
    fn next(&mut self, el: E);
    fn value(&self) -> Option<Self::Output>;
}

pub struct Sum<E> {
    queue: VecDeque<E>,
    sum: Option<E>,
    size: usize,
}

impl<E> Indicator<E> for Sum<E>
where
    E: Add<Output = E> + Sub<Output = E> + Copy,
{
    type Output = E;
    fn new(size: usize) -> Result<Self, &'static str> {
        return if size < 1 {
            Err("Size cannot be smaller than 1!")
        } else {
            Ok(Sum {
                queue: VecDeque::with_capacity(size),
                sum: None,
                size,
            })
        };
    }
    fn next(&mut self, el: E) {
        self.sum = Some(if let Some(sum) = self.sum {
            sum + if self.queue.len() < self.size {
                el
            } else {
                el - self.queue.pop_back().unwrap()
            }
        } else {
            el
        });
        self.queue.push_front(el);
    }
    fn value(&self) -> Option<E> {
        return self.sum;
    }
}

pub struct Average<E>
where
    E: Dividable,
{
    sum: Sum<E>,
    len: E::Divider,
}

impl<E> Indicator<E> for Average<E>
where
    E: Dividable + Copy + Add<Output = E> + Sub<Output = E>,
    E::Divider: Identity + Add<Output = E::Divider> + Copy,
{
    type Output = E;
    fn new(size: usize) -> Result<Self, &'static str> {
        let sum = Sum::new(size)?;
        return Ok(Average {
            sum,
            len: E::Divider::zero(),
        });
    }
    fn next(&mut self, el: E) {
        if self.sum.queue.len() < self.sum.size {
            self.len = self.len + E::Divider::one();
        }
        self.sum.next(el);
    }
    fn value(&self) -> Option<E> {
        let sum = self.sum.value()?;
        return Some(sum / self.len);
    }
}

pub struct Variance<E>
where
    E: Dividable,
{
    avg: Average<E>,
    sum: Option<E>,
}

impl<E> Variance<E>
where
    E: Dividable + Copy + Add<Output = E> + Sub<Output = E>,
    E::Divider: Identity + Add<Output = E::Divider> + Copy,
{
    pub fn average(&self) -> Option<E> {
        return self.avg.value();
    }
}

impl<E> Indicator<E> for Variance<E>
where
    E: Dividable + Copy + Add<Output = E> + Sub<Output = E> + Mul<Output = E>,
    E::Divider: Identity + Add<Output = E::Divider> + Sub<Output = E::Divider> + Copy,
{
    type Output = E;
    fn new(size: usize) -> Result<Self, &'static str> {
        let avg = Average::new(size)?;
        return Ok(Variance { avg, sum: None });
    }
    fn next(&mut self, el: E) {
        self.sum = if let Some(old_avg) = self.avg.value() {
            let last_el = *self.avg.sum.queue.back().unwrap();
            let old_len = self.avg.sum.queue.len();
            self.avg.next(el);
            let avg = self.avg.value().unwrap();
            let sum = if old_len == self.avg.sum.size {
                (el - avg + last_el - old_avg) * (el - last_el)
            } else {
                (el - avg) * (el - old_avg)
            };
            Some(if let Some(old_sum) = self.sum {
                sum + old_sum
            } else {
                sum
            })
        } else {
            self.avg.next(el);
            None
        };
    }
    fn value(&self) -> Option<E> {
        let sum = self.sum?;
        return Some(sum / (self.avg.len - E::Divider::one()));
    }
}

pub struct Covariance<E>
where
    E: Dividable,
{
    x_avg: Average<E>,
    y_avg: Average<E>,
    sum: Option<E>,
}

impl<E> Indicator<(E, E)> for Covariance<E>
where
    E: Dividable + Copy + Add<Output = E> + Sub<Output = E> + Mul<Output = E>,
    E::Divider: Identity + Add<Output = E::Divider> + Sub<Output = E::Divider> + Copy,
{
    type Output = E;
    fn new(size: usize) -> Result<Self, &'static str> {
        let x_avg = Average::new(size)?;
        let y_avg = Average::new(size)?;
        return Ok(Covariance {
            x_avg,
            y_avg,
            sum: None,
        });
    }
    fn next(&mut self, (x, y): (E, E)) {
        self.sum = if let Some(old_x_avg) = self.x_avg.value() {
            let last_x = *self.x_avg.sum.queue.back().unwrap();
            let last_y = *self.y_avg.sum.queue.back().unwrap();
            self.x_avg.next(x);
            self.y_avg.next(y);
            let y_avg = self.y_avg.value().unwrap();
            let sum = if self.x_avg.sum.queue.len() == self.x_avg.sum.size {
                (x - old_x_avg) * (y - y_avg) - (last_x - old_x_avg) * (last_y - y_avg)
            } else {
                (x - old_x_avg) * (y - y_avg)
            };
            Some(if let Some(old_sum) = self.sum {
                sum + old_sum
            } else {
                sum
            })
        } else {
            self.x_avg.next(x);
            self.y_avg.next(y);
            None
        };
    }
    fn value(&self) -> Option<E> {
        let sum = self.sum?;
        return Some(sum / (self.x_avg.len - E::Divider::one()));
    }
}

pub struct LinearRegression<E> 
where
    E: Dividable
{
    cov: Covariance<E>,
    var: Variance<E>
}

impl<E> Indicator<(E, E)> for LinearRegression<E>
where
    E: Dividable + Copy + Add<Output = E> + Sub<Output = E> + Mul<Output = E> + Div<E, Output = E>,
    E::Divider: Identity + Add<Output = E::Divider> + Sub<Output = E::Divider> + Copy,
{
    type Output = (E, E);
    fn new(size: usize) -> Result<Self, &'static str> {
        let var = Variance::new(size)?;
        let cov = Covariance::new(size)?;
        return Ok(LinearRegression{cov, var})
    }
    fn next(&mut self, el: (E, E)) {
        self.var.next(el.0);
        self.cov.next(el);
    }
    fn value(&self) -> Option<(E, E)> {
        let var = self.var.sum?;
        let cov = self.cov.sum?;
        let s_x = self.cov.x_avg.sum.value()?;
        let s_y = self.cov.y_avg.sum.value()?;
        let b = cov/var;
        let a = (s_y - b * s_x) / self.var.avg.len;
        Some((a, b))
    }
}

/*MIT*/
// Based on https://github.com/craffel/median-filter/blob/master/Mediator.h by Colin Raffel
// Original under MIT license. For posterity following code between /*MIT*/ markers can be
// considered dual licensed under MIT and GPLv3+.

pub struct Median<E> {
    data: Vec<E>,
    pos: Vec<isize>,
    allocated_heap: Vec<usize>,
    size: usize,
    min_ct: isize,
    max_ct: isize,
    idx: usize,
}

impl<E> Median<E>
where
    E: PartialOrd,
{
    fn heap(&self, i: isize) -> usize {
        return self.allocated_heap[(i + (self.size / 2) as isize) as usize];
    }
    fn heap_set(&mut self, i: isize, el: usize) {
        self.allocated_heap[(i + (self.size / 2) as isize) as usize] = el;
    }
    fn less(&self, i: isize, j: isize) -> bool {
        return self.data[self.heap(i)] < self.data[self.heap(j)];
    }
    fn exchange(&mut self, i: isize, j: isize) {
        let heap_i = self.heap(i);
        let heap_j = self.heap(j);
        self.heap_set(i, heap_j);
        self.heap_set(j, heap_i);
        self.pos[heap_i] = j;
        self.pos[heap_j] = i;
    }
    fn cmp_exch(&mut self, i: isize, j: isize) -> bool {
        return if self.less(i, j) {
            self.exchange(i, j);
            true
        } else {
            false
        };
    }
    fn min_sort_up(&mut self, mut i: isize) -> bool {
        while i > 0 && self.cmp_exch(i, i / 2) {
            i /= 2;
        }
        return i == 0;
    }
    fn min_sort_down(&mut self, mut i: isize) {
        while {
            i *= 2;
            i <= self.min_ct
        } {
            if i < self.min_ct && self.less(i + 1, i) {
                i += 1;
            }
            if !self.cmp_exch(i, i / 2) {
                break;
            }
        }
    }
    fn max_sort_up(&mut self, mut i: isize) -> bool {
        while i < 0 && self.cmp_exch(i / 2, i) {
            i /= 2;
        }
        return i == 0;
    }
    fn max_sort_down(&mut self, mut i: isize) {
        while {
            i *= 2;
            i >= -self.max_ct
        } {
            if i > -self.max_ct && self.less(i, i - 1) {
                i -= 1;
            }
            if !self.cmp_exch(i / 2, i) {
                break;
            }
        }
    }
}

impl<E> Indicator<E> for Median<E>
where
    E: PartialOrd + Add<Output = E> + Copy + Dividable,
    E::Divider: Identity + Add<Output = E::Divider> + Copy,
{
    type Output = E;
    fn new(size: usize) -> Result<Self, &'static str> {
        if size < 1 || size == usize::MAX {
            return Err("Size cannot be smaller than 1 or equal to usize::MAX!");
        }
        let mut out = Median {
            data: Vec::with_capacity(size),
            pos: Vec::with_capacity(size),
            allocated_heap: vec![0; size],
            size,
            min_ct: 0,
            max_ct: 0,
            idx: 0,
        };
        for idx in 0..size {
            let el = ((idx + 1) / 2) as isize * if idx & 1 == 0 { 1 } else { -1 };
            out.pos.push(el);
            out.heap_set(el, idx);
        }
        return Ok(out);
    }
    fn next(&mut self, el: E) {
        let p = self.pos[self.idx];
        let mut old = None;
        if self.data.len() <= self.idx {
            self.data.push(el);
        } else {
            old = Some(self.data[self.idx]);
            self.data[self.idx] = el;
        }
        self.idx = (self.idx + 1) % self.size;
        if p > 0 {
            if self.min_ct < ((self.size - 1) / 2) as isize {
                self.min_ct += 1;
            } else if el > old.unwrap() {
                self.min_sort_down(p);
                return;
            }
            if self.min_sort_up(p) && self.cmp_exch(0, -1) {
                self.max_sort_down(-1);
            }
        } else if p < 0 {
            if self.max_ct < (self.size / 2) as isize {
                self.max_ct += 1;
            } else if el < old.unwrap() {
                self.max_sort_down(p);
                return;
            }
            if self.max_sort_up(p) && self.min_ct != 0 && self.cmp_exch(1, 0) {
                self.min_sort_down(1);
            }
        } else {
            if self.max_ct != 0 && self.max_sort_up(-1) {
                self.max_sort_down(-1);
            }
            if self.min_ct != 0 && self.min_sort_up(1) {
                self.min_sort_down(1);
            }
        }
    }
    fn value(&self) -> Option<E> {
        return if self.data.len() == 0 {
            None
        } else {
            let el = self.data[self.heap(0)];
            Some(if self.min_ct < self.max_ct {
                (el + self.data[self.heap(-1)]) / (E::Divider::one() + E::Divider::one())
            } else {
                el
            })
        };
    }
}
/*MIT*/

#[cfg(test)]
mod tests {
    use rand::prelude::*;
    use super::*;
    use std::collections::VecDeque;
    const SIZE: usize = 10000;
    const ITERS: usize = 10;
    type TYPE = f64;
    const EPS: TYPE = 1e-9;

    macro_rules! test_indicator {
        ($ind:ident, $lval:expr) => {
            let mut rng = rand::thread_rng();
            let mut test_queue = VecDeque::<TYPE>::with_capacity(SIZE);
            let mut test_indicator = $ind::new(SIZE).unwrap();

            let mut max_err = TYPE::zero();

            for iter in 0..ITERS {
                for el in 0..SIZE {
                    let val = rng.gen();
                    test_queue.push_front(val);
                    if iter > 0 {
                        test_queue.pop_back();
                    }
                    test_indicator.next(val);
                    let lval: TYPE = $lval(&test_queue);
                    if let Some(rval) = test_indicator.value() {
                        let err = (lval - rval).abs();
                        max_err = if err > max_err { err } else { max_err };
                        assert!(err < EPS, "{} is not equal to {} within tolerance ({}), after {} operations.", lval, rval, EPS, iter*SIZE + el);
                    }
                }
            }
            println!("Max Error: {}", max_err);
        }
    }

    #[test]
    fn test_sum() {
        test_indicator!(Sum, |tq: &VecDeque<TYPE>| {
            tq.iter().sum()
        });
    }

    #[test]
    fn test_average() {
        test_indicator!(Average, |tq: &VecDeque<TYPE>| {
            tq.iter().sum::<TYPE>()/(tq.len() as TYPE)
        });
    }
    #[test]
    fn test_variance() {
        test_indicator!(Variance, |tq: &VecDeque<TYPE>| {
            let len = tq.len() as TYPE;
            let mean = tq.iter().sum::<TYPE>()/len;
            tq.iter().fold(0., |acc, el| {
                acc + (el - mean).powi(2)
            })/(len - 1.)

        });
    }
    #[test]
    fn test_covariance() {
    }
    #[test]
    fn test_linear_regression() {
    }
    #[test]
    fn test_median() {
    }
}
