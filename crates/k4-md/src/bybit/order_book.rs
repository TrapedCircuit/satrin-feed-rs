//! Incremental order book maintaining up to N price levels.
//!
//! Bybit uses incremental updates for `orderbook.50` — an initial snapshot is
//! followed by delta messages that add, update, or remove levels. This module
//! maintains the full sorted book and can extract the top 5 levels as a
//! [`Depth5`]-compatible tuple.

/// Incremental order book maintaining up to `N` price levels per side.
///
/// - Bids are sorted **descending** by price (best bid first).
/// - Asks are sorted **ascending** by price (best ask first).
///
/// # Const parameter
///
/// `N` is the maximum number of levels to retain. For Bybit `orderbook.50`,
/// use `OrderBook<50>`.
pub struct OrderBook<const N: usize> {
    /// Bid levels `[price, volume]`, sorted descending by price.
    bids: Vec<[f64; 2]>,
    /// Ask levels `[price, volume]`, sorted ascending by price.
    asks: Vec<[f64; 2]>,
}

/// Tolerance for floating-point price comparison.
const PRICE_EPS: f64 = 1e-10;

impl<const N: usize> OrderBook<N> {
    /// Create a new empty order book.
    pub fn new() -> Self {
        Self {
            bids: Vec::with_capacity(N),
            asks: Vec::with_capacity(N),
        }
    }

    /// Replace the entire book with a snapshot.
    ///
    /// Both `bids` and `asks` are `[price, volume]` pairs. They are re-sorted
    /// internally (bids descending, asks ascending) and trimmed to `N` levels.
    pub fn set_snapshot(&mut self, bids: &[[f64; 2]], asks: &[[f64; 2]]) {
        self.bids.clear();
        self.bids.extend_from_slice(&bids[..bids.len().min(N)]);
        self.bids
            .sort_by(|a, b| b[0].partial_cmp(&a[0]).unwrap_or(std::cmp::Ordering::Equal));

        self.asks.clear();
        self.asks.extend_from_slice(&asks[..asks.len().min(N)]);
        self.asks
            .sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
    }

    /// Apply an incremental delta to the book.
    ///
    /// For each `[price, volume]` pair:
    /// - If `volume == 0.0`, the level at that price is **removed**.
    /// - If the price already exists, the volume is **updated**.
    /// - Otherwise, a new level is **inserted** at the correct sorted position.
    ///
    /// After insertion, if the book exceeds `N` levels the worst level is
    /// trimmed (highest ask / lowest bid).
    pub fn update(&mut self, bids: &[[f64; 2]], asks: &[[f64; 2]]) {
        for &[price, vol] in bids {
            update_side_desc(&mut self.bids, price, vol, N);
        }
        for &[price, vol] in asks {
            update_side_asc(&mut self.asks, price, vol, N);
        }
    }

    /// Extract the top 5 levels from each side.
    ///
    /// Returns `(bid_prices, bid_vols, ask_prices, ask_vols, bid_level, ask_level)`.
    pub fn get_depth5(&self) -> ([f64; 5], [f64; 5], [f64; 5], [f64; 5], u32, u32) {
        let mut bid_prices = [0.0f64; 5];
        let mut bid_vols = [0.0f64; 5];
        let mut ask_prices = [0.0f64; 5];
        let mut ask_vols = [0.0f64; 5];

        let bid_levels = self.bids.len().min(5);
        let ask_levels = self.asks.len().min(5);

        for i in 0..bid_levels {
            bid_prices[i] = self.bids[i][0];
            bid_vols[i] = self.bids[i][1];
        }
        for i in 0..ask_levels {
            ask_prices[i] = self.asks[i][0];
            ask_vols[i] = self.asks[i][1];
        }

        (
            bid_prices,
            bid_vols,
            ask_prices,
            ask_vols,
            bid_levels as u32,
            ask_levels as u32,
        )
    }

    /// Returns `true` if the book has no levels on either side.
    pub fn is_empty(&self) -> bool {
        self.bids.is_empty() && self.asks.is_empty()
    }
}

impl<const N: usize> Default for OrderBook<N> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Update a bid side (sorted **descending** by price).
fn update_side_desc(levels: &mut Vec<[f64; 2]>, price: f64, vol: f64, max_levels: usize) {
    // Search for existing level at this price.
    if let Some(idx) = levels.iter().position(|l| (l[0] - price).abs() < PRICE_EPS) {
        if vol == 0.0 {
            levels.remove(idx);
        } else {
            levels[idx][1] = vol;
        }
    } else if vol > 0.0 {
        // Insert at correct position (descending order — higher prices first).
        let pos = levels
            .iter()
            .position(|l| l[0] < price)
            .unwrap_or(levels.len());
        levels.insert(pos, [price, vol]);
        if levels.len() > max_levels {
            levels.pop(); // Remove worst (lowest) bid
        }
    }
}

/// Update an ask side (sorted **ascending** by price).
fn update_side_asc(levels: &mut Vec<[f64; 2]>, price: f64, vol: f64, max_levels: usize) {
    if let Some(idx) = levels.iter().position(|l| (l[0] - price).abs() < PRICE_EPS) {
        if vol == 0.0 {
            levels.remove(idx);
        } else {
            levels[idx][1] = vol;
        }
    } else if vol > 0.0 {
        // Insert at correct position (ascending order — lower prices first).
        let pos = levels
            .iter()
            .position(|l| l[0] > price)
            .unwrap_or(levels.len());
        levels.insert(pos, [price, vol]);
        if levels.len() > max_levels {
            levels.pop(); // Remove worst (highest) ask
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_and_depth5() {
        let mut book = OrderBook::<50>::new();
        book.set_snapshot(
            &[
                [100.0, 1.0],
                [99.0, 2.0],
                [98.0, 3.0],
                [97.0, 4.0],
                [96.0, 5.0],
                [95.0, 6.0],
            ],
            &[
                [101.0, 1.0],
                [102.0, 2.0],
                [103.0, 3.0],
                [104.0, 4.0],
                [105.0, 5.0],
                [106.0, 6.0],
            ],
        );

        let (bp, bv, ap, av, bl, al) = book.get_depth5();
        assert_eq!(bl, 5);
        assert_eq!(al, 5);
        assert!((bp[0] - 100.0).abs() < PRICE_EPS);
        assert!((ap[0] - 101.0).abs() < PRICE_EPS);
        assert!((bv[0] - 1.0).abs() < PRICE_EPS);
        assert!((av[0] - 1.0).abs() < PRICE_EPS);
    }

    #[test]
    fn incremental_update() {
        let mut book = OrderBook::<50>::new();
        book.set_snapshot(&[[100.0, 1.0], [99.0, 2.0]], &[[101.0, 1.0], [102.0, 2.0]]);

        // Update: change volume at 100.0, add new level at 100.5
        book.update(&[[100.0, 5.0], [100.5, 3.0]], &[]);

        let (bp, bv, _, _, bl, _) = book.get_depth5();
        assert_eq!(bl, 3);
        // Best bid should be 100.5 (newly inserted)
        assert!((bp[0] - 100.5).abs() < PRICE_EPS);
        assert!((bv[0] - 3.0).abs() < PRICE_EPS);
        // Second should be 100.0 (updated volume)
        assert!((bp[1] - 100.0).abs() < PRICE_EPS);
        assert!((bv[1] - 5.0).abs() < PRICE_EPS);
    }

    #[test]
    fn remove_level() {
        let mut book = OrderBook::<50>::new();
        book.set_snapshot(&[[100.0, 1.0], [99.0, 2.0]], &[[101.0, 1.0]]);

        // Remove bid at 100.0 (vol=0)
        book.update(&[[100.0, 0.0]], &[]);

        let (bp, bv, _, _, bl, _) = book.get_depth5();
        assert_eq!(bl, 1);
        assert!((bp[0] - 99.0).abs() < PRICE_EPS);
        assert!((bv[0] - 2.0).abs() < PRICE_EPS);
    }
}
