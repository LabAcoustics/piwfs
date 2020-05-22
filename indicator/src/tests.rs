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
    let mut rng = rand::thread_rng();
    let mut test_queue = VecDeque::<(TYPE, TYPE)>::with_capacity(SIZE);
    let mut test_indicator = Covariance::new(SIZE).unwrap();

    let mut max_err = TYPE::zero();

    for iter in 0..ITERS {
        for el in 0..SIZE {
            let val1 = rng.gen();
            let val2 = rng.gen();
            test_queue.push_front((val1, val2));
            if iter > 0 {
                test_queue.pop_back();
            }
            test_indicator.next((val1, val2));
            let len = test_queue.len() as TYPE;
            let (sum1, sum2) = test_queue.iter().fold((0., 0.), |acc, (el1, el2)| {
                (acc.0 + el1, acc.1 + el2)
            });
            let (mean1, mean2) = (sum1/len, sum2/len);
            let lval: TYPE = test_queue.iter().fold(0., |acc, (el1, el2)| {
                acc + (el1 - mean1)*(el2 - mean2)
            })/(len - 1.);
            if let Some(rval) = test_indicator.value() {
                let err = (lval - rval).abs();
                max_err = if err > max_err { err } else { max_err };
                assert!(err < EPS, "{} is not equal to {} within tolerance ({}), after {} operations.", lval, rval, EPS, iter*SIZE + el);
            }
        }
    }
    println!("Max Error: {}", max_err);
}
#[test]
fn test_linear_regression() {
    let mut rng = rand::thread_rng();
    let mut test_queue = VecDeque::<(TYPE, TYPE)>::with_capacity(SIZE);
    let mut test_indicator = LinearRegression::new(SIZE).unwrap();

    let mut max_err = (TYPE::zero(), TYPE::zero());

    for iter in 0..ITERS {
        for el in 0..SIZE {
            let x = rng.gen();
            let y = rng.gen();
            test_queue.push_front((x, y));
            if iter > 0 {
                test_queue.pop_back();
            }
            test_indicator.next((x, y));
            let len = test_queue.len() as TYPE;
            let (sxy, sx, sy, sxx) = test_queue.iter().fold((0., 0., 0., 0.), |acc, (x1, y1)| {
                (acc.0 + x1*y1, acc.1 + x1, acc.2 + y1, acc.3 + x1*x1)
            });
            let l_b = (sxy - (sx*sy)/len)/(sxx - (sx*sx)/len);
            let l_a = sy/len - l_b*sx/len;
            if let Some((r_a, r_b)) = test_indicator.value() {
                let err_a = (l_a - r_a).abs();
                let err_b = (l_b - r_b).abs();
                max_err.0 = if err_a > max_err.0 { err_a } else  { max_err.0 };
                max_err.1 = if err_b > max_err.1 { err_b } else  { max_err.1 };
                assert!(err_b < EPS, "B: {} is not equal to {} within tolerance ({}), after {} operations.", l_b, r_b, EPS, iter*SIZE + el);
                assert!(err_a < EPS, "A: {} is not equal to {} within tolerance ({}), after {} operations.", l_a, r_a, EPS, iter*SIZE + el);
            }
        }
    }
    println!("Max Error: {:?}", max_err);
}
#[test]
fn test_median() {
    const SIZE: usize = 1000;
    test_indicator!(Median, |tq: &VecDeque<TYPE>| {
        let len = tq.len();
        let mut tqvec: Vec<TYPE> = tq.iter().map(|el| *el).collect();
        tqvec.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if len % 2 == 0 {
            (tqvec[len/2] + tqvec[len/2 - 1])/(TYPE::one() + TYPE::one())
        } else {
            tqvec[len/2]
        }
    });
}
