use crate::{
    field::Field,
    sbpir::{query::Queriable, ExposeOffset, StepType, StepTypeUUID, PIR, SBPIR},
    util::{uuid, UUID},
    wit_gen::{FixedGenContext, StepInstance, TraceContext},
};

use halo2_proofs::plonk::{Advice, Column as Halo2Column, Fixed};

use core::{fmt::Debug, hash::Hash};
use std::marker::PhantomData;

use self::{
    cb::{eq, Constraint, Typing},
    lb::{LookupBuilder, LookupTable, LookupTableRegistry, LookupTableStore},
};

pub use sc::*;

#[derive(Debug)]
/// A generic structure designed to handle the context of a circuit for generic types
/// `F`, `TraceArgs` and `StepArgs`.
/// The struct contains a `Circuit` instance and implements methods to build the circuit,
/// add various components, and manipulate the circuit.
/// `F` is a generic type representing the field of the circuit.
/// `TraceArgs` is a generic type representing the arguments passed to the trace function.
/// `StepArgs` is a generic type representing the arguments passed to the `step_type_def` function.
pub struct CircuitContext<F, TraceArgs> {
    circuit: SBPIR<F, TraceArgs>,
    tables: LookupTableRegistry<F>,
}

impl<F, TraceArgs> CircuitContext<F, TraceArgs> {
    /// Adds a forward signal to the circuit with a name string and zero rotation and returns a
    /// `Queriable` instance representing the added forward signal.
    pub fn forward(&mut self, name: &str) -> Queriable<F> {
        Queriable::Forward(self.circuit.add_forward(name, 0), false)
    }

    /// Adds a forward signal to the circuit with a name string and a specified phase and returns a
    /// `Queriable` instance representing the added forward signal.
    pub fn forward_with_phase(&mut self, name: &str, phase: usize) -> Queriable<F> {
        Queriable::Forward(self.circuit.add_forward(name, phase), false)
    }

    /// Adds a shared signal to the circuit with a name string and zero rotation and returns a
    /// `Queriable` instance representing the added shared signal.
    pub fn shared(&mut self, name: &str) -> Queriable<F> {
        Queriable::Shared(self.circuit.add_shared(name, 0), 0)
    }

    /// Adds a shared signal to the circuit with a name string and a specified phase and returns a
    /// `Queriable` instance representing the added shared signal.
    pub fn shared_with_phase(&mut self, name: &str, phase: usize) -> Queriable<F> {
        Queriable::Shared(self.circuit.add_shared(name, phase), 0)
    }

    pub fn fixed(&mut self, name: &str) -> Queriable<F> {
        Queriable::Fixed(self.circuit.add_fixed(name), 0)
    }

    /// Exposes the first step instance value of a forward signal as public.
    pub fn expose(&mut self, queriable: Queriable<F>, offset: ExposeOffset) {
        self.circuit.expose(queriable, offset);
    }

    /// Imports a halo2 advice column with a name string into the circuit and returns a
    /// `Queriable` instance representing the imported column.
    pub fn import_halo2_advice(&mut self, name: &str, column: Halo2Column<Advice>) -> Queriable<F> {
        Queriable::Halo2AdviceQuery(self.circuit.add_halo2_advice(name, column), 0)
    }

    /// Imports a halo2 fixed column with a name string into the circuit and returns a
    /// `Queriable` instance representing the imported column.
    pub fn import_halo2_fixed(&mut self, name: &str, column: Halo2Column<Fixed>) -> Queriable<F> {
        Queriable::Halo2FixedQuery(self.circuit.add_halo2_fixed(name, column), 0)
    }

    /// Adds a new step type with the specified name to the circuit and returns a
    /// `StepTypeHandler` instance. The `StepTypeHandler` instance can be used to define the
    /// step type using the `step_type_def` function.
    pub fn step_type(&mut self, name: &str) -> StepTypeHandler {
        let handler = StepTypeHandler::new(name.to_string());

        self.circuit.add_step_type(handler, name);

        handler
    }

    /// Defines a step type using the provided `StepTypeHandler` and a function that takes a
    /// mutable reference to a `StepTypeContext`. This function typically adds constraints to a
    /// step type and defines witness generation.
    pub fn step_type_def<D, Args, S: Into<StepTypeDefInput>, R>(
        &mut self,
        step: S,
        def: D,
    ) -> StepTypeWGHandler<F, Args, R>
    where
        D: FnOnce(&mut StepTypeContext<F>) -> StepTypeWGHandler<F, Args, R>,
        R: Fn(&mut StepInstance<F>, Args) + 'static,
    {
        let handler: StepTypeHandler = match step.into() {
            StepTypeDefInput::Handler(h) => h,
            StepTypeDefInput::String(name) => {
                let handler = StepTypeHandler::new(name.to_string());

                self.circuit.add_step_type(handler, name);

                handler
            }
        };

        let mut context = StepTypeContext::<F>::new(
            handler.uuid(),
            handler.annotation.to_string(),
            self.tables.clone(),
        );

        let result = def(&mut context);

        self.circuit.add_step_type_def(context.step_type);

        result
    }

    /// Sets the trace function that builds the witness. The trace function is responsible for
    /// adding step instances defined in `step_type_def`. The function is entirely left for
    /// the user to implement and is Turing complete. Users typically use external parameters
    /// of type `TraceArgs` to generate cell values for witness generation, and call the
    /// `add` function to add step instances with witness values.
    pub fn trace<D>(&mut self, def: D)
    where
        D: Fn(&mut TraceContext<F>, TraceArgs) + 'static,
    {
        self.circuit.set_trace(def);
    }

    pub fn new_table(&self, table: LookupTableStore<F>) -> LookupTable {
        let uuid = table.uuid();
        self.tables.add(table);

        LookupTable { uuid }
    }

    /// Enforce the type of the first step by adding a constraint to the circuit. Takes a
    /// `StepTypeHandler` parameter that represents the step type.
    pub fn pragma_first_step<STH: Into<StepTypeHandler>>(&mut self, step_type: STH) {
        self.circuit.first_step = Some(step_type.into().uuid());
    }

    /// Enforce the type of the last step by adding a constraint to the circuit. Takes a
    /// `StepTypeHandler` parameter that represents the step type.
    pub fn pragma_last_step<STH: Into<StepTypeHandler>>(&mut self, step_type: STH) {
        self.circuit.last_step = Some(step_type.into().uuid());
    }

    pub fn pragma_num_steps(&mut self, num_steps: usize) {
        self.circuit.num_steps = num_steps;
    }

    pub fn pragma_disable_q_enable(&mut self) {
        self.circuit.q_enable = false;
    }
}

impl<F: Field + Hash, TraceArgs> CircuitContext<F, TraceArgs> {
    /// Executes the fixed generation function provided by the user and sets the fixed assignments
    /// for the circuit. The fixed generation function is responsible for assigning fixed values to
    /// fixed columns. It is entirely left for the user to implement and is Turing complete. Users
    /// typically generate cell values and call the `assign` function to fill the fixed columns.
    pub fn fixed_gen<D>(&mut self, def: D)
    where
        D: Fn(&mut FixedGenContext<F>) + 'static,
    {
        if self.circuit.num_steps == 0 {
            panic!("circuit must call pragma_num_steps before calling fixed_gen");
        }
        let mut ctx = FixedGenContext::new(self.circuit.num_steps);
        (def)(&mut ctx);

        let assignments = ctx.get_assignments();

        self.circuit.set_fixed_assignments(assignments);
    }
}

pub enum StepTypeDefInput {
    Handler(StepTypeHandler),
    String(&'static str),
}

impl From<StepTypeHandler> for StepTypeDefInput {
    fn from(h: StepTypeHandler) -> Self {
        StepTypeDefInput::Handler(h)
    }
}

impl From<&'static str> for StepTypeDefInput {
    fn from(s: &'static str) -> Self {
        StepTypeDefInput::String(s)
    }
}

/// A generic structure designed to handle the context of a step type definition.  The struct
/// contains a `StepType` instance and implements methods to build the step type, add components,
/// and manipulate the step type. `F` is a generic type representing the field of the step type.
/// `Args` is the type of the step instance witness generation arguments.
pub struct StepTypeContext<F> {
    step_type: StepType<F>,
    tables: LookupTableRegistry<F>,
}

impl<F> StepTypeContext<F> {
    pub fn new(uuid: UUID, name: String, tables: LookupTableRegistry<F>) -> Self {
        Self {
            step_type: StepType::new(uuid, name),
            tables,
        }
    }

    /// Adds an internal signal to the step type with the given name and returns a `Queriable`
    /// instance representing the added signal.
    pub fn internal(&mut self, name: &str) -> Queriable<F> {
        Queriable::Internal(self.step_type.add_signal(name))
    }

    /// DEPRECATED
    pub fn constr<C: Into<Constraint<F>>>(&mut self, constraint: C) {
        println!("DEPRECATED constr: use setup for constraints in step types");

        let constraint = constraint.into();

        self.step_type
            .add_constr(constraint.annotation, constraint.expr);
    }

    /// DEPRECATED
    pub fn transition<C: Into<Constraint<F>>>(&mut self, constraint: C) {
        println!("DEPRECATED transition: use setup for constraints in step types");

        let constraint = constraint.into();

        self.step_type
            .add_transition(constraint.annotation, constraint.expr);
    }

    /// Define step constraints.
    pub fn setup<D>(&mut self, def: D)
    where
        D: Fn(&mut StepTypeSetupContext<F>),
    {
        let mut ctx = StepTypeSetupContext {
            step_type: &mut self.step_type,
            tables: self.tables.clone(),
        };

        def(&mut ctx);
    }

    pub fn wg<Args, D>(&mut self, def: D) -> StepTypeWGHandler<F, Args, D>
    where
        D: Fn(&mut StepInstance<F>, Args) + 'static,
    {
        StepTypeWGHandler {
            id: self.step_type.uuid(),
            annotation: Box::leak(self.step_type.name.clone().into_boxed_str()),

            wg: Box::new(def),

            _p: PhantomData,
        }
    }
}

pub struct StepTypeSetupContext<'a, F> {
    step_type: &'a mut StepType<F>,
    tables: LookupTableRegistry<F>,
}

impl<'a, F> StepTypeSetupContext<'a, F> {
    /// Adds a constraint to the step type. Involves internal signal(s) and forward signals without
    /// SuperRotation only. Chiquito provides syntax sugar for defining complex constraints.
    /// Refer to the `cb` (constraint builder) module for more information.
    pub fn constr<C: Into<Constraint<F>>>(&mut self, constraint: C) {
        let constraint = constraint.into();
        Self::enforce_constraint_typing(&constraint);

        self.step_type
            .add_constr(constraint.annotation, constraint.expr);
    }

    /// Adds a transition constraint to the step type. It’s the same as a regular constraint except
    /// that it can involve forward signal(s) with SuperRotation as well. Chiquito provides syntax
    /// sugar for defining complex constraints. Refer to the `cb` (constraint builder) module
    /// for more information.
    pub fn transition<C: Into<Constraint<F>>>(&mut self, constraint: C) {
        let constraint = constraint.into();
        Self::enforce_constraint_typing(&constraint);

        self.step_type
            .add_transition(constraint.annotation, constraint.expr);
    }

    fn enforce_constraint_typing(constraint: &Constraint<F>) {
        if constraint.typing != Typing::AntiBooly {
            panic!(
                "Expected AntiBooly constraint, got {:?} (constraint: {})",
                constraint.typing, constraint.annotation
            );
        }
    }
}

impl<'a, F: Eq + PartialEq + Hash + Debug + Clone> StepTypeSetupContext<'a, F> {
    pub fn auto(&mut self, signal: Queriable<F>, expr: PIR<F>) {
        self.step_type.auto_signals.insert(signal, expr);
    }

    pub fn auto_eq(&mut self, signal: Queriable<F>, expr: PIR<F>) {
        self.auto(signal.clone(), expr.clone());

        self.constr(eq(signal, expr));
    }
}

impl<'a, F: Debug + Clone> StepTypeSetupContext<'a, F> {
    /// Adds a lookup to the step type.
    pub fn add_lookup<LB: LookupBuilder<F>>(&mut self, lookup_builder: LB) {
        self.step_type.lookups.push(lookup_builder.build(self));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StepTypeHandler {
    id: StepTypeUUID,
    pub annotation: &'static str,
}

impl StepTypeHandler {
    fn new(annotation: String) -> Self {
        Self {
            id: uuid(),
            annotation: Box::leak(annotation.into_boxed_str()),
        }
    }

    pub fn new_with_id(id: UUID, annotation: String) -> Self {
        Self {
            id,
            annotation: Box::leak(annotation.into_boxed_str()),
        }
    }

    pub fn uuid(&self) -> UUID {
        self.id
    }

    pub fn next<F>(&self) -> Queriable<F> {
        Queriable::StepTypeNext(*self)
    }
}

impl<F, Args, D: Fn(&mut StepInstance<F>, Args) + 'static> From<&StepTypeWGHandler<F, Args, D>>
    for StepTypeHandler
{
    fn from(h: &StepTypeWGHandler<F, Args, D>) -> Self {
        StepTypeHandler {
            id: h.id,
            annotation: h.annotation,
        }
    }
}

pub struct StepTypeWGHandler<F, Args, D: Fn(&mut StepInstance<F>, Args) + 'static> {
    id: UUID,
    pub annotation: &'static str,
    pub wg: Box<D>,

    _p: PhantomData<(F, Args)>,
}

impl<F, Args, D: Fn(&mut StepInstance<F>, Args) + 'static> StepTypeWGHandler<F, Args, D> {
    pub fn new(id: UUID, annotation: &'static str, wg: D) -> Self {
        StepTypeWGHandler {
            id,
            annotation,
            wg: Box::new(wg),
            _p: PhantomData,
        }
    }

    pub fn uuid(&self) -> UUID {
        self.id
    }
}

/// Creates a `Circuit` instance by providing a name and a definition closure that is applied to a
/// mutable `CircuitContext`. The user customizes the definition closure by calling `CircuitContext`
/// functions. This is the main function that users call to define a Chiquito circuit. Currently,
/// the name is not used for annotation within the function, but it may be used in future
/// implementations.
pub fn circuit<F, TraceArgs, D>(_name: &str, def: D) -> SBPIR<F, TraceArgs>
where
    D: Fn(&mut CircuitContext<F, TraceArgs>),
{
    // TODO annotate circuit
    let mut context = CircuitContext {
        circuit: SBPIR::default(),
        tables: LookupTableRegistry::default(),
    };

    def(&mut context);

    context.circuit
}

pub mod cb;
pub mod lb;
pub mod sc;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disable_q_enable() {
        let circuit: SBPIR<i32, i32> = SBPIR::default();
        let mut context = CircuitContext {
            circuit,
            tables: Default::default(),
        };

        context.pragma_disable_q_enable();

        assert!(!context.circuit.q_enable);
    }

    #[test]
    fn test_set_num_steps() {
        let circuit: SBPIR<i32, i32> = SBPIR::default();
        let mut context = CircuitContext {
            circuit,
            tables: Default::default(),
        };

        context.pragma_num_steps(3);
        assert_eq!(context.circuit.num_steps, 3);

        context.pragma_num_steps(0);
        assert_eq!(context.circuit.num_steps, 0);
    }

    #[test]
    fn test_forward() {
        // create circuit context
        let circuit: SBPIR<i32, i32> = SBPIR::default();
        let mut context = CircuitContext {
            circuit,
            tables: Default::default(),
        };

        // set forward signals
        let forward_a: Queriable<i32> = context.forward("forward_a");
        let forward_b: Queriable<i32> = context.forward("forward_b");

        // assert forward signals are correct
        assert_eq!(context.circuit.forward_signals.len(), 2);
        assert_eq!(context.circuit.forward_signals[0].uuid(), forward_a.uuid());
        assert_eq!(context.circuit.forward_signals[1].uuid(), forward_b.uuid());
    }

    #[test]
    fn test_forward_with_phase() {
        // create circuit context
        let circuit: SBPIR<i32, i32> = SBPIR::default();
        let mut context = CircuitContext {
            circuit,
            tables: Default::default(),
        };

        // set forward signals with specified phase
        context.forward_with_phase("forward_a", 1);
        context.forward_with_phase("forward_b", 2);

        // assert forward signals are correct
        assert_eq!(context.circuit.forward_signals.len(), 2);
        assert_eq!(context.circuit.forward_signals[0].phase(), 1);
        assert_eq!(context.circuit.forward_signals[1].phase(), 2);
    }

    #[test]
    fn test_shared() {
        // create circuit context
        let circuit: SBPIR<i32, i32> = SBPIR::default();
        let mut context = CircuitContext {
            circuit,
            tables: Default::default(),
        };

        // set shared signal
        let shared_a: Queriable<i32> = context.shared("shared_a");

        // assert shared signal is correct
        assert_eq!(context.circuit.shared_signals.len(), 1);
        assert_eq!(context.circuit.shared_signals[0].uuid(), shared_a.uuid());
    }

    #[test]
    fn test_shared_with_phase() {
        // create circuit context
        let circuit: SBPIR<i32, i32> = SBPIR::default();
        let mut context = CircuitContext {
            circuit,
            tables: Default::default(),
        };

        // set shared signal with specified phase
        context.shared_with_phase("shared_a", 2);

        // assert shared signal is correct
        assert_eq!(context.circuit.shared_signals.len(), 1);
        assert_eq!(context.circuit.shared_signals[0].phase(), 2);
    }

    #[test]
    fn test_fixed() {
        // create circuit context
        let circuit: SBPIR<i32, i32> = SBPIR::default();
        let mut context = CircuitContext {
            circuit,
            tables: Default::default(),
        };

        // set fixed signal
        context.fixed("fixed_a");

        // assert fixed signal was added to the circuit
        assert_eq!(context.circuit.fixed_signals.len(), 1);
    }

    #[test]
    fn test_expose() {
        // create circuit context
        let circuit: SBPIR<i32, i32> = SBPIR::default();
        let mut context = CircuitContext {
            circuit,
            tables: Default::default(),
        };

        // set forward signal and step to expose
        let forward_a: Queriable<i32> = context.forward("forward_a");
        let step_offset: ExposeOffset = ExposeOffset::Last;

        // expose the forward signal of the final step
        context.expose(forward_a, step_offset);

        // assert the signal is exposed
        assert_eq!(context.circuit.exposed[0].0, forward_a);
        assert_eq!(
            std::mem::discriminant(&context.circuit.exposed[0].1),
            std::mem::discriminant(&step_offset)
        );
    }

    #[test]
    fn test_step_type() {
        // create circuit context
        let circuit: SBPIR<i32, i32> = SBPIR::default();
        let mut context = CircuitContext {
            circuit,
            tables: Default::default(),
        };

        // create a step type
        let handler: StepTypeHandler = context.step_type("fibo_first_step");

        // assert that the created step type was added to the circuit annotations
        assert_eq!(
            context.circuit.annotations[&handler.uuid()],
            "fibo_first_step"
        );
    }

    #[test]
    fn test_step_type_def() {
        // create circuit context
        let circuit: SBPIR<i32, i32> = SBPIR::default();
        let mut context = CircuitContext {
            circuit,
            tables: Default::default(),
        };

        // create a step type including its definition
        let simple_step = context.step_type_def("simple_step", |context| {
            context.setup(|_| {});
            context.wg(|_, _: u32| {})
        });

        // assert step type was created and added to the circuit
        assert_eq!(
            context.circuit.annotations[&simple_step.uuid()],
            "simple_step"
        );
        assert_eq!(
            simple_step.uuid(),
            context.circuit.step_types[&simple_step.uuid()].uuid()
        );
    }

    #[test]
    fn test_step_type_def_pass_handler() {
        // create circuit context
        let circuit: SBPIR<i32, i32> = SBPIR::default();
        let mut context = CircuitContext {
            circuit,
            tables: Default::default(),
        };

        // create a step type handler
        let handler: StepTypeHandler = context.step_type("simple_step");

        // create a step type including its definition
        let simple_step = context.step_type_def(handler, |context| {
            context.setup(|_| {});
            context.wg(|_, _: u32| {})
        });

        // assert step type was created and added to the circuit
        assert_eq!(
            context.circuit.annotations[&simple_step.uuid()],
            "simple_step"
        );
        assert_eq!(
            simple_step.uuid(),
            context.circuit.step_types[&simple_step.uuid()].uuid()
        );
    }
}
