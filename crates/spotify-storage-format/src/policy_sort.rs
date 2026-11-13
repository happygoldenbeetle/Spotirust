// Copyright (c) Microsoft Corporation.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
// Modified: Rust specialization for native cache deadline ordering.
// Based on Microsoft STL vs-2022-17.8 algorithm, _Sort_unchecked and partition
// helpers: https://github.com/microsoft/STL/blob/vs-2022-17.8/stl/inc/algorithm
// See LICENSE-MICROSOFT-STL.txt. Matching its swaps matters when deadlines tie.

type Item = (usize, i64);

pub(crate) fn sort(items: &mut [Item]) {
    sort_range(items, 0, items.len(), items.len());
}

fn med3(a: &mut [Item], first: usize, mid: usize, last: usize) {
    if a[mid].1 < a[first].1 {
        a.swap(mid, first);
    }
    if a[last].1 < a[mid].1 {
        a.swap(last, mid);
        if a[mid].1 < a[first].1 {
            a.swap(mid, first);
        }
    }
}

fn partition(a: &mut [Item], first: usize, last: usize) -> (usize, usize) {
    let mid = first + (last - first) / 2;
    let inclusive_last = last - 1;
    if inclusive_last - first > 40 {
        let step = (last - first) / 8;
        med3(a, first, first + step, first + 2 * step);
        med3(a, mid - step, mid, mid + step);
        med3(
            a,
            inclusive_last - 2 * step,
            inclusive_last - step,
            inclusive_last,
        );
        med3(a, first + step, mid, inclusive_last - step);
    } else {
        med3(a, first, mid, inclusive_last);
    }
    let mut pfirst = mid;
    let mut plast = mid + 1;
    while first < pfirst && a[pfirst - 1].1 == a[pfirst].1 {
        pfirst -= 1;
    }
    while plast < last && a[plast].1 == a[pfirst].1 {
        plast += 1;
    }
    let mut gfirst = plast;
    let mut glast = pfirst;
    loop {
        while gfirst < last {
            if a[pfirst].1 < a[gfirst].1 {
                gfirst += 1;
                continue;
            }
            if a[gfirst].1 < a[pfirst].1 {
                break;
            }
            if plast != gfirst {
                a.swap(plast, gfirst);
            }
            plast += 1;
            gfirst += 1;
        }
        while first < glast {
            let previous = glast - 1;
            if a[previous].1 < a[pfirst].1 {
                glast -= 1;
                continue;
            }
            if a[pfirst].1 < a[previous].1 {
                break;
            }
            pfirst -= 1;
            if pfirst != previous {
                a.swap(pfirst, previous);
            }
            glast -= 1;
        }
        if glast == first && gfirst == last {
            return (pfirst, plast);
        }
        if glast == first {
            if plast != gfirst {
                a.swap(pfirst, plast);
            }
            plast += 1;
            a.swap(pfirst, gfirst);
            pfirst += 1;
            gfirst += 1;
        } else if gfirst == last {
            glast -= 1;
            pfirst -= 1;
            if glast != pfirst {
                a.swap(glast, pfirst);
            }
            plast -= 1;
            a.swap(pfirst, plast);
        } else {
            glast -= 1;
            a.swap(gfirst, glast);
            gfirst += 1;
        }
    }
}

fn sort_range(a: &mut [Item], mut first: usize, mut last: usize, mut ideal: usize) {
    loop {
        if last - first <= 32 {
            for i in first..last {
                let value = a[i];
                let mut hole = i;
                while hole > first && value.1 < a[hole - 1].1 {
                    a[hole] = a[hole - 1];
                    hole -= 1;
                }
                a[hole] = value;
            }
            return;
        }
        if ideal == 0 {
            heap_sort(&mut a[first..last]);
            return;
        }
        let (pfirst, plast) = partition(a, first, last);
        ideal = (ideal >> 1) + (ideal >> 2);
        if pfirst - first < last - plast {
            sort_range(a, first, pfirst, ideal);
            first = plast;
        } else {
            sort_range(a, plast, last, ideal);
            last = pfirst;
        }
    }
}

fn adjust_heap(a: &mut [Item], mut hole: usize, count: usize, value: Item) {
    let top = hole;
    let mut child = 2 * hole + 2;
    while child < count {
        if a[child].1 < a[child - 1].1 {
            child -= 1;
        }
        a[hole] = a[child];
        hole = child;
        child = 2 * hole + 2;
    }
    if child == count {
        a[hole] = a[child - 1];
        hole = child - 1;
    }
    while hole > top {
        let parent = (hole - 1) / 2;
        if a[parent].1 >= value.1 {
            break;
        }
        a[hole] = a[parent];
        hole = parent;
    }
    a[hole] = value;
}
fn heap_sort(a: &mut [Item]) {
    for hole in (0..a.len() / 2).rev() {
        adjust_heap(a, hole, a.len(), a[hole]);
    }
    for last in (1..a.len()).rev() {
        let value = a[last];
        a[last] = a[0];
        adjust_heap(a, 0, last, value);
    }
}
