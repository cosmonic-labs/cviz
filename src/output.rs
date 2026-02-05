/// Output format for visualization
#[derive(Debug, Clone, Copy, Default)]
pub enum OutputFormat {
    #[default]
    Ascii,
    Mermaid,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ascii" => Ok(OutputFormat::Ascii),
            "mermaid" => Ok(OutputFormat::Mermaid),
            _ => Err(format!("Invalid output format: {}. Valid values: ascii, mermaid", s)),
        }
    }
}

/// Diagram direction (applies to Mermaid only)
#[derive(Debug, Clone, Copy, Default)]
pub enum Direction {
    #[default]
    LeftToRight,
    TopDown,
}

impl Direction {
    pub fn to_mermaid(&self) -> &'static str {
        match self {
            Direction::LeftToRight => "LR",
            Direction::TopDown => "TD",
        }
    }
}

impl std::str::FromStr for Direction {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "lr" | "left-to-right" => Ok(Direction::LeftToRight),
            "td" | "top-down" => Ok(Direction::TopDown),
            _ => Err(format!("Invalid direction: {}", s)),
        }
    }
}

/// Detail level for the diagram
#[derive(Debug, Clone, Copy, Default)]
pub enum DetailLevel {
    /// Only show the HTTP handler chain
    #[default]
    HandlerChain,
    /// Show all interfaces
    AllInterfaces,
    /// Show everything including internal details
    Full,
}

impl std::str::FromStr for DetailLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "handler-chain" | "handler" => Ok(DetailLevel::HandlerChain),
            "all-interfaces" | "all" => Ok(DetailLevel::AllInterfaces),
            "full" => Ok(DetailLevel::Full),
            _ => Err(format!("Invalid detail level: {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_format_parse() {
        assert!(matches!("ascii".parse::<OutputFormat>().unwrap(), OutputFormat::Ascii));
        assert!(matches!("mermaid".parse::<OutputFormat>().unwrap(), OutputFormat::Mermaid));
        assert!("invalid".parse::<OutputFormat>().is_err());
    }

    #[test]
    fn test_direction_parse() {
        assert!(matches!("lr".parse::<Direction>().unwrap(), Direction::LeftToRight));
        assert!(matches!("td".parse::<Direction>().unwrap(), Direction::TopDown));
    }

    #[test]
    fn test_detail_level_parse() {
        assert!(matches!("handler-chain".parse::<DetailLevel>().unwrap(), DetailLevel::HandlerChain));
        assert!(matches!("all-interfaces".parse::<DetailLevel>().unwrap(), DetailLevel::AllInterfaces));
        assert!(matches!("full".parse::<DetailLevel>().unwrap(), DetailLevel::Full));
    }
}
