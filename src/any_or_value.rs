use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnyOr<T> {
    Any,
    Specific(T),
}

impl<T> AnyOr<T>
where
    T: Clone,
{
    pub fn coalesce(items: &[AnyOr<T>]) -> AnyOr<Vec<T>> {
        let mut ret = vec![];

        for item in items {
            match item {
                AnyOr::Any => return AnyOr::Any,
                AnyOr::Specific(v) => ret.push(v.clone()),
            }
        }

        AnyOr::Specific(ret)
    }
}

impl<T> FromStr for AnyOr<T>
where
    T: FromStr,
{
    type Err = <T as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "*" => Ok(Self::Any),
            s => s.parse().map(AnyOr::Specific),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test_case("*" => AnyOr::Any)]
    #[test_case("123" => AnyOr::Specific(123))]
    fn parsing(s: &str) -> AnyOr<i32> {
        s.parse().unwrap()
    }

    #[test_case(&["*", "1", "2"] => AnyOr::Any)]
    #[test_case(&["47", "2"] => AnyOr::Specific(vec![47, 2]))]
    #[test_case(&["1", "*"] => AnyOr::Any)]
    fn coalesce(items: &[&str]) -> AnyOr<Vec<i32>> {
        let items: Vec<AnyOr<i32>> = items.iter().map(|s| s.parse().unwrap()).collect();

        AnyOr::coalesce(items)
    }
}
