use std::collections::VecDeque;
use std::ops::{Add, Div, Mul, Sub};

pub trait Identity {
    fn zero() -> Self;
    fn one() -> Self;
}

macro_rules! impl_identity {
    ($Z:expr, $O:expr, $($T:ty),*) => (
        $(
            impl Identity for $T {
                fn zero() -> Self { $Z }
                fn one() -> Self { $O }
            }
        )*
    )
}

impl_identity!(0, 1, u8, u16, u32, u64, u128, i8, i16, i32, i64, i128);
impl_identity!(0., 1., f32, f64);

pub trait Indicator<E>
where
    Self: Sized,
{
    fn new(size: usize) -> Result<Self, &'static str>;
    fn next(&mut self, el: E) -> E;
    fn value(&self) -> Option<E>;
}

pub struct SimpleMovingAverage<E> {
    queue: VecDeque<E>,
    len: E,
    sum: Option<E>,
    size: usize,
}

impl<E> Indicator<E> for SimpleMovingAverage<E>
where
    E: Identity + Div<Output = E> + Add<Output = E> + Sub<Output = E> + Copy,
{
    fn new(size: usize) -> Result<Self, &'static str> {
        if size < 1 {
            return Err("Size cannot be smaller than 1!");
        }
        return Ok(SimpleMovingAverage {
            queue: VecDeque::with_capacity(size),
            len: E::zero(),
            sum: None,
            size,
        });
    }
    fn next(&mut self, el: E) -> E {
        let mut sum = if let Some(old_sum) = self.sum {
            old_sum + el
        } else {
            el
        };
        if self.queue.len() == self.size {
            sum = sum - self.queue.pop_back().unwrap()
        } else if self.queue.len() < self.size {
            self.len = self.len + E::one();
        } else {
            unreachable!()
        }
        self.queue.push_front(el);
        self.sum = Some(sum);
        return sum / self.len;
    }
    fn value(&self) -> Option<E> {
        return if let Some(sum) = self.sum {
            Some(sum / self.len)
        } else {
            None
        };
    }
}

pub struct WelfordsMovingVariance<E> {
    sma: SimpleMovingAverage<E>,
    var_sum: Option<E>,
}

impl<E> WelfordsMovingVariance<E>
where
    E: Identity + Div<Output = E> + Add<Output = E> + Sub<Output = E> + Copy,
{
    pub fn average(&self) -> Option<E> {
        return self.sma.value();
    }
}

impl<E> Indicator<E> for WelfordsMovingVariance<E>
where
    E: Identity
        + Div<Output = E>
        + Add<Output = E>
        + Sub<Output = E>
        + Mul<Output = E>
        + Copy
        + PartialOrd,
{
    fn new(size: usize) -> Result<Self, &'static str> {
        let sma = SimpleMovingAverage::new(size)?;
        return Ok(WelfordsMovingVariance { sma, var_sum: None });
    }
    fn next(&mut self, el: E) -> E {
        let mut sum = if let Some(old_sum) = self.var_sum {
            let last_el = *self.sma.queue.back().unwrap();
            let old_avg = self.sma.value().unwrap();
            let len = self.sma.queue.len();
            let avg = self.sma.next(el);
            old_sum
                + if len == self.sma.size {
                    (el - avg + last_el - old_avg) * (el - last_el)
                } else {
                    (el - avg) * (el - old_avg)
                }
        } else {
            self.sma.next(el);
            E::zero()
        };
        sum = if sum < E::zero() { E::zero() } else { sum };
        self.var_sum = Some(sum);
        return if self.sma.len > E::one() {
            sum / (self.sma.len - E::one())
        } else {
            E::zero()
        };
    }
    fn value(&self) -> Option<E> {
        return if let Some(sum) = self.var_sum {
            if self.sma.len > E::one() {
                Some(sum / (self.sma.len - E::one()))
            } else {
                Some(E::zero())
            }
        } else {
            None
        };
    }
}

pub struct OrdinaryLeastSquares<E> {
    queue: VecDeque<(E, E)>,
    s_x: E,
    s_y: E,
    s_xx: E,
    s_yy: E,
    s_xy: E,
    len: E,
    size: usize,
}

impl<E> Indicator<(E, E)> for OrdinaryLeastSquares<E> 
where
    E: Identity + PartialOrd
        + Div<Output = E>
        + Add<Output = E>
        + Sub<Output = E>
        + Mul<Output = E>
        + Copy
{
    fn new(size: usize) -> Result<Self, &'static str> {
        if size < 1 {
            return Err("Size cannot be smaller than 1!");
        }
        Ok(OrdinaryLeastSquares {
            len: E::zero(),
            queue: VecDeque::with_capacity(size),
            size,
            s_x: E::zero(),
            s_y: E::zero(),
            s_xx: E::zero(),
            s_yy: E::zero(),
            s_xy: E::zero()
        })
    }
    fn next(&mut self, el: (E, E)) -> (E, E) {
        let (x, y) = el;
        if self.queue.len() < self.size {
            self.len = self.len + E::one();
            self.s_x = self.s_x + x;
            self.s_y = self.s_y + y;
            self.s_xx = self.s_xx + x*x;
            self.s_yy = self.s_yy + y*y;
            self.s_xy = self.s_xy + x*y;
        } else {
            let (old_x, old_y) = self.queue.pop_back().unwrap();
            self.s_x = self.s_x + x - old_x;
            self.s_y = self.s_y + y - old_y;
            self.s_xx = self.s_xx + (x - old_x)*(x + old_x);
            self.s_yy = self.s_yy + (y - old_y)*(y + old_y);
            self.s_xy = self.s_xy + x*y - old_x*old_y;
        }
        self.queue.push_front(el);
        return self.value().unwrap();
    }
    fn value(&self) -> Option<(E, E)> {
        if self.len > E::zero() {
            let b = (self.len*self.s_xy - self.s_x*self.s_y)/(self.len*self.s_xx - self.s_x*self.s_x);
            let a = (self.s_y - b*self.s_x)/self.len;
            Some((a, b))
        } else {
            None
        }
    }
}

/*MIT*/
// Based on https://github.com/craffel/median-filter/blob/master/Mediator.h by Colin Raffel
// Original under MIT license. For posterity following code between /*MIT*/ markers can be
// considered dual licensed under MIT and GPLv3+.

pub struct SlidingMedianFilter<E> {
    data: Vec<E>,
    pos: Vec<isize>,
    allocated_heap: Vec<usize>,
    size: usize,
    min_ct: isize,
    max_ct: isize,
    idx: usize,
}

impl<E> SlidingMedianFilter<E>
where
    E: PartialOrd,
{
    fn heap(&self, i: isize) -> usize {
        return self.allocated_heap[(i + (self.size/2) as isize) as usize]
    }
    
    fn heap_set(&mut self, i: isize, el: usize) {
        self.allocated_heap[(i + (self.size/2) as isize) as usize] = el;
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
        return if self.less(i, j)  {
            self.exchange(i, j);
            true
        } else {
            false
        }
    }

    fn min_sort_up(&mut self, mut i: isize) -> bool {
        while i > 0 && self.cmp_exch(i, i/2) {
            i /= 2;
        }
        return i == 0;
    }

    fn min_sort_down(&mut self, mut i: isize) {
        while {i *= 2; i <= self.min_ct} {
            if i < self.min_ct && self.less(i+1, i) {
                i += 1;
            }
            if !self.cmp_exch(i, i/2) {
                break
            }
        }

    }

    fn max_sort_up(&mut self, mut i: isize) -> bool {
        while i < 0 && self.cmp_exch(i/2, i) {
            i /= 2;
        }
        return i == 0;
    }

    fn max_sort_down(&mut self, mut i: isize) {
        while {i *= 2; i >= -self.max_ct} {
            if i > -self.max_ct && self.less(i, i-1) {
                i -= 1;
            }
            if !self.cmp_exch(i/2, i) {
                break
            }
        }
    }
}

impl<E> Indicator<E> for SlidingMedianFilter<E> 
where
    E: PartialOrd
        + Div<Output = E>
        + Add<Output = E>
        + Identity
        + Copy
{
    fn new(size: usize) -> Result<Self, &'static str> {
        if size < 1 || size == usize::MAX {
            return Err("Size cannot be smaller than 1 or equal to usize::MAX!");
        }
        let mut out = SlidingMedianFilter {
            data: Vec::with_capacity(size),
            pos: Vec::with_capacity(size),
            allocated_heap: vec![0; size],
            size,
            min_ct: 0,
            max_ct: 0,
            idx: 0,
        };
        for idx in 0..size {
            let el = ((idx + 1)/2) as isize * if idx & 1 == 0 { 1 } else { -1 };
            out.pos.push(el);
            out.heap_set(el, idx);
        }
        return Ok(out)
    }
    fn next(&mut self, el: E) -> E {
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
            if self.min_ct < ((self.size-1)/2) as isize {
                self.min_ct += 1;
            } else if el > old.unwrap() {
                self.min_sort_down(p);
                return self.value().unwrap();
            }
            if self.min_sort_up(p) && self.cmp_exch(0, -1) {
                self.max_sort_down(-1);
            }
        } else if p < 0 {
            if self.max_ct < (self.size/2) as isize {
                self.max_ct += 1;
            } else if el < old.unwrap() {
                self.max_sort_down(p);
                return self.value().unwrap();
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
        return self.value().unwrap()
    }
    fn value(&self) -> Option<E> {
        return if self.data.len() == 0 {
             None
        } else {
            let el = self.data[self.heap(0)];
            Some(if self.min_ct < self.max_ct {
                (el + self.data[self.heap(-1)]) / (E::one() + E::one())
            } else {
                el
            })
        }
    }
}
/*MIT*/
