//! Semantic analysis of the Tx3 language.
//!
//! This module takes an AST and performs semantic analysis on it. It checks for
//! duplicate definitions, unknown symbols, and other semantic errors.

use std::{collections::HashMap, rc::Rc};
use thiserror::Error;

use crate::ast::*;

#[derive(Debug, thiserror::Error, miette::Diagnostic)]
#[error("not in scope: {name}")]
#[diagnostic(code(tx3::not_in_scope))]
pub struct NotInScopeError {
    pub name: String,

    #[source_code]
    src: Option<String>,

    #[label]
    span: Span,
}

#[derive(Debug, thiserror::Error, miette::Diagnostic)]
#[error("invalid symbol, expected {expected}, got {got}")]
#[diagnostic(code(tx3::invalid_symbol))]
pub struct InvalidSymbolError {
    pub expected: &'static str,
    pub got: String,

    #[source_code]
    src: Option<String>,

    #[label]
    span: Span,
}

#[derive(Error, Debug, miette::Diagnostic)]
pub enum Error {
    #[error("duplicate definition: {0}")]
    #[diagnostic(code(tx3::duplicate_definition))]
    DuplicateDefinition(String),

    #[error(transparent)]
    #[diagnostic(transparent)]
    NotInScope(#[from] NotInScopeError),

    #[error("needs parent scope")]
    #[diagnostic(code(tx3::needs_parent_scope))]
    NeedsParentScope,

    #[error(transparent)]
    #[diagnostic(transparent)]
    InvalidSymbol(#[from] InvalidSymbolError),
}

impl Error {
    pub fn span(&self) -> &Span {
        match self {
            Self::NotInScope(x) => &x.span,
            Self::InvalidSymbol(x) => &x.span,
            _ => &Span::DUMMY,
        }
    }

    pub fn src(&self) -> Option<&str> {
        match self {
            Self::NotInScope(x) => x.src.as_deref(),
            _ => None,
        }
    }

    pub fn not_in_scope(name: String, ast: &impl crate::parsing::AstNode) -> Self {
        Self::NotInScope(NotInScopeError {
            name,
            src: None,
            span: ast.span().clone(),
        })
    }

    pub fn invalid_symbol(
        expected: &'static str,
        got: &Symbol,
        ast: &impl crate::parsing::AstNode,
    ) -> Self {
        Self::InvalidSymbol(InvalidSymbolError {
            expected,
            got: format!("{:?}", got),
            src: None,
            span: ast.span().clone(),
        })
    }
}

#[derive(Debug, Default)]
pub struct AnalyzeReport {
    pub errors: Vec<Error>,
}

impl AnalyzeReport {
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn ok(self) -> Result<(), Self> {
        if self.is_empty() {
            Ok(())
        } else {
            Err(self)
        }
    }
}

impl std::error::Error for AnalyzeReport {}

impl std::fmt::Display for AnalyzeReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AnalyzeReport {{ errors: {:?} }}", self.errors)
    }
}

impl std::ops::Add for Error {
    type Output = AnalyzeReport;

    fn add(self, other: Self) -> Self::Output {
        Self::Output {
            errors: vec![self, other],
        }
    }
}

impl From<Error> for AnalyzeReport {
    fn from(error: Error) -> Self {
        Self {
            errors: vec![error],
        }
    }
}

impl From<Vec<Error>> for AnalyzeReport {
    fn from(errors: Vec<Error>) -> Self {
        Self { errors }
    }
}

impl std::ops::Add for AnalyzeReport {
    type Output = AnalyzeReport;

    fn add(self, other: Self) -> Self::Output {
        [self, other].into_iter().collect()
    }
}

impl FromIterator<Error> for AnalyzeReport {
    fn from_iter<T: IntoIterator<Item = Error>>(iter: T) -> Self {
        Self {
            errors: iter.into_iter().collect(),
        }
    }
}

impl FromIterator<AnalyzeReport> for AnalyzeReport {
    fn from_iter<T: IntoIterator<Item = AnalyzeReport>>(iter: T) -> Self {
        Self {
            errors: iter.into_iter().flat_map(|r| r.errors).collect(),
        }
    }
}

macro_rules! bail_report {
    ($($args:expr),*) => {
        { return AnalyzeReport::from(vec![$($args),*]); }
    };
}

impl Scope {
    pub fn new(parent: Option<Rc<Scope>>) -> Self {
        Self {
            symbols: HashMap::new(),
            parent,
        }
    }

    pub fn track_type_def(&mut self, type_: &TypeDef) {
        self.symbols
            .insert(type_.name.clone(), Symbol::TypeDef(Box::new(type_.clone())));
    }

    pub fn track_variant_case(&mut self, case: &VariantCase) {
        self.symbols.insert(
            case.name.clone(),
            Symbol::VariantCase(Box::new(case.clone())),
        );
    }

    pub fn track_record_field(&mut self, field: &RecordField) {
        self.symbols.insert(
            field.name.clone(),
            Symbol::RecordField(Box::new(field.clone())),
        );
    }

    pub fn track_party_def(&mut self, party: &PartyDef) {
        self.symbols.insert(
            party.name.clone(),
            Symbol::PartyDef(Box::new(party.clone())),
        );
    }

    pub fn track_policy_def(&mut self, policy: &PolicyDef) {
        self.symbols.insert(
            policy.name.clone(),
            Symbol::PolicyDef(Box::new(policy.clone())),
        );
    }

    pub fn track_asset_def(&mut self, asset: &AssetDef) {
        self.symbols.insert(
            asset.name.clone(),
            Symbol::AssetDef(Box::new(asset.clone())),
        );
    }

    pub fn track_param_var(&mut self, param: &str, r#type: Type) {
        self.symbols.insert(
            param.to_string(),
            Symbol::ParamVar(param.to_string(), Box::new(r#type)),
        );
    }

    pub fn track_input(&mut self, input: &InputBlock) {
        self.symbols
            .insert(input.name.clone(), Symbol::Input(input.name.clone()));
    }

    pub fn resolve(&self, name: &str) -> Option<Symbol> {
        if let Some(symbol) = self.symbols.get(name) {
            Some(symbol.clone())
        } else if let Some(parent) = &self.parent {
            parent.resolve(name)
        } else {
            None
        }
    }
}

/// A trait for types that can be semantically analyzed.
///
/// Types implementing this trait can validate their semantic correctness and
/// resolve symbol references within a given scope.
pub trait Analyzable {
    /// Performs semantic analysis on the type.
    ///
    /// # Arguments
    /// * `parent` - Optional parent scope containing symbol definitions
    ///
    /// # Returns
    /// * `AnalyzeReport` of the analysis. Empty if no errors are found.
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport;
}

impl<T: Analyzable> Analyzable for Option<T> {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        if let Some(item) = self {
            item.analyze(parent)
        } else {
            AnalyzeReport::default()
        }
    }
}

impl<T: Analyzable> Analyzable for Box<T> {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        self.as_mut().analyze(parent)
    }
}

impl<T: Analyzable> Analyzable for Vec<T> {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        self.iter_mut()
            .map(|item| item.analyze(parent.clone()))
            .collect()
    }
}

impl Analyzable for PolicyField {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        match self {
            PolicyField::Hash(x) => x.analyze(parent),
            PolicyField::Script(x) => x.analyze(parent),
            PolicyField::Ref(x) => x.analyze(parent),
        }
    }
}
impl Analyzable for PolicyConstructor {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        self.fields.analyze(parent)
    }
}

impl Analyzable for PolicyDef {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        match &mut self.value {
            PolicyValue::Constructor(x) => x.analyze(parent),
            PolicyValue::Assign(_) => AnalyzeReport::default(),
        }
    }
}

impl Analyzable for DataBinaryOp {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        let left = self.left.analyze(parent.clone());
        let right = self.right.analyze(parent.clone());

        left + right
    }
}

impl Analyzable for RecordConstructorField {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        let name = self.name.analyze(parent.clone());
        let value = self.value.analyze(parent.clone());

        name + value
    }
}

impl Analyzable for VariantCaseConstructor {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        let name = self.name.analyze(parent.clone());

        let mut scope = Scope::new(parent);

        let case = match &self.name.symbol {
            Some(Symbol::VariantCase(x)) => x,
            Some(x) => bail_report!(Error::invalid_symbol("VariantCase", x, &self.name)),
            _ => unreachable!(),
        };

        for field in case.fields.iter() {
            scope.track_record_field(field);
        }

        self.scope = Some(Rc::new(scope));

        let fields = self.fields.analyze(self.scope.clone());

        name + fields
    }
}

impl Analyzable for DatumConstructor {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        let r#type = self.r#type.analyze(parent.clone());

        let mut scope = Scope::new(parent);

        let type_def = match &self.r#type.symbol {
            Some(Symbol::TypeDef(x)) => x,
            Some(x) => bail_report!(Error::invalid_symbol("TypeDef", x, &self.r#type)),
            _ => unreachable!(),
        };

        for case in type_def.cases.iter() {
            scope.track_variant_case(case);
        }

        self.scope = Some(Rc::new(scope));

        let case = self.case.analyze(self.scope.clone());

        r#type + case
    }
}

impl Analyzable for DataExpr {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        match self {
            DataExpr::Constructor(x) => x.analyze(parent),
            DataExpr::Identifier(x) => x.analyze(parent),
            DataExpr::PropertyAccess(x) => x.analyze(parent),
            DataExpr::BinaryOp(x) => x.analyze(parent),
            _ => AnalyzeReport::default(),
        }
    }
}

impl Analyzable for AssetBinaryOp {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        let left = self.left.analyze(parent.clone());
        let right = self.right.analyze(parent.clone());

        left + right
    }
}

impl Analyzable for AssetConstructor {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        let amount = self.amount.analyze(parent.clone());
        let r#type = self.r#type.analyze(parent.clone());

        amount + r#type
    }
}

impl Analyzable for PropertyAccess {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        let object = self.object.analyze(parent.clone());

        self.scope = Some(Rc::new(Scope::new(parent)));

        let path = self.path.analyze(self.scope.clone());

        object + path
    }
}

impl Analyzable for AssetExpr {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        match self {
            AssetExpr::Identifier(x) => x.analyze(parent),
            AssetExpr::Constructor(x) => x.analyze(parent),
            AssetExpr::BinaryOp(x) => x.analyze(parent),
            AssetExpr::PropertyAccess(x) => x.analyze(parent),
        }
    }
}

impl Analyzable for AddressExpr {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        match self {
            AddressExpr::Identifier(x) => x.analyze(parent),
            _ => AnalyzeReport::default(),
        }
    }
}
impl Analyzable for Identifier {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        let symbol = parent.and_then(|p| p.resolve(&self.value));

        if symbol.is_none() {
            bail_report!(Error::not_in_scope(self.value.clone(), self));
        }

        self.symbol = symbol;

        AnalyzeReport::default()
    }
}

impl Analyzable for Type {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        match self {
            Type::Custom(x) => x.analyze(parent),
            _ => AnalyzeReport::default(),
        }
    }
}

impl Analyzable for InputBlockField {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        match self {
            InputBlockField::From(x) => x.analyze(parent),
            InputBlockField::DatumIs(x) => x.analyze(parent),
            InputBlockField::MinAmount(x) => x.analyze(parent),
            InputBlockField::Redeemer(x) => x.analyze(parent),
            InputBlockField::Ref(x) => x.analyze(parent),
        }
    }
}

impl Analyzable for InputBlock {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        self.fields.analyze(parent)
    }
}

impl Analyzable for OutputBlockField {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        match self {
            OutputBlockField::To(x) => x.analyze(parent),
            OutputBlockField::Amount(x) => x.analyze(parent),
            OutputBlockField::Datum(x) => x.analyze(parent),
        }
    }
}

impl Analyzable for OutputBlock {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        self.fields.analyze(parent)
    }
}

impl Analyzable for RecordField {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        self.r#type.analyze(parent)
    }
}

impl Analyzable for VariantCase {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        self.fields.analyze(parent)
    }
}

impl Analyzable for TypeDef {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        self.cases.analyze(parent)
    }
}

impl Analyzable for MintBlockField {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        match self {
            MintBlockField::Amount(x) => x.analyze(parent),
            MintBlockField::Redeemer(x) => x.analyze(parent),
        }
    }
}

impl Analyzable for MintBlock {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        self.fields.analyze(parent)
    }
}

impl Analyzable for ChainSpecificBlock {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        match self {
            ChainSpecificBlock::Cardano(x) => x.analyze(parent),
        }
    }
}

impl Analyzable for TxDef {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        let mut scope = Scope::new(parent);

        scope.symbols.insert("fees".to_string(), Symbol::Fees);

        for param in self.parameters.parameters.iter() {
            scope.track_param_var(&param.name, param.r#type.clone());
        }

        for input in self.inputs.iter() {
            scope.track_input(input);
        }

        self.scope = Some(Rc::new(scope));

        let inputs = self.inputs.analyze(self.scope.clone());

        let outputs = self.outputs.analyze(self.scope.clone());

        let mint = self.mint.analyze(self.scope.clone());

        let adhoc = self.adhoc.analyze(self.scope.clone());

        inputs + outputs + mint + adhoc
    }
}

static ADA: std::sync::LazyLock<AssetDef> = std::sync::LazyLock::new(|| AssetDef {
    name: "Ada".to_string(),
    policy: HexStringLiteral::new("".to_string()),
    asset_name: "".to_string(),
    span: Span::DUMMY,
});

impl Analyzable for Program {
    fn analyze(&mut self, parent: Option<Rc<Scope>>) -> AnalyzeReport {
        let mut scope = Scope::new(parent);

        for party in self.parties.iter() {
            scope.track_party_def(party);
        }

        for policy in self.policies.iter() {
            scope.track_policy_def(policy);
        }

        scope.track_asset_def(&ADA);

        for asset in self.assets.iter() {
            scope.track_asset_def(asset);
        }

        for type_def in self.types.iter() {
            scope.track_type_def(type_def);
        }

        self.scope = Some(Rc::new(scope));

        // TODO: Add parties
        // let parties = self.parties.analyze(self.scope.clone());

        let policies = self.policies.analyze(self.scope.clone());

        // TODO: Add assets
        // let assets = self.assets.analyze(self.scope.clone());

        let types = self.types.analyze(self.scope.clone());

        let txs = self.txs.analyze(self.scope.clone());

        policies + types + txs
    }
}

/// Performs semantic analysis on a Tx3 program AST.
///
/// This function validates the entire program structure, checking for:
/// - Duplicate definitions
/// - Unknown symbol references
/// - Type correctness
/// - Other semantic constraints
///
/// # Arguments
/// * `ast` - Mutable reference to the program AST to analyze
///
/// # Returns
/// * `AnalyzeReport` of the analysis. Empty if no errors are found.
pub fn analyze(ast: &mut Program) -> AnalyzeReport {
    ast.analyze(None)
}
