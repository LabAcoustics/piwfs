use std::collections::VecDeque;
use std::ops::{Add, Div, Sub, Mul};

pub trait Indicator<E> where Self: Sized {
    fn new(size: usize) -> Result<Self, &'static str>;
    fn next(&mut self, el: E) -> E;
    fn value(&self) -> Option<E>;
}

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

pub struct SimpleMovingAverage<E> {
    queue: VecDeque<E>,
    len: E,
    sum: Option<E>,
    size: usize
}

pub struct WelfordsMovingVariance<E> {
    sma: SimpleMovingAverage<E>,
    var_sum: Option<E>
}

impl<E> WelfordsMovingVariance<E>
where
    E: Identity
        + Div<Output = E>
        + Add<Output = E>
        + Sub<Output = E>
        + Copy
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
        + PartialOrd
{
    fn new(size: usize) -> Result<Self, &'static str> {
        let sma = SimpleMovingAverage::new(size)?;
        return Ok(WelfordsMovingVariance {
            sma,
            var_sum: None
        });
    }
    fn next(&mut self, el: E) -> E {
        let mut sum = if let Some(old_sum) = self.var_sum {
            let last_el = *self.sma.queue.back().unwrap();
            let old_avg = self.sma.value().unwrap();
            let len = self.sma.queue.len();
            let avg = self.sma.next(el);
            old_sum + if len == self.sma.size {
                (el - avg + last_el - old_avg)*(el - last_el)
            } else {
                (el - avg)*(el - old_avg)
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
        }
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

impl<E> Indicator<E> for SimpleMovingAverage<E>
where
    E: Identity
        + Div<Output = E>
        + Add<Output = E>
        + Sub<Output = E>
        + Copy
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
        return sum / self.len
    }
    fn value(&self) -> Option<E> {
        return if let Some(sum) = self.sum {
            Some(sum / self.len)
        } else {
            None
        };
    }
}
