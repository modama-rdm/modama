// I just know the version used by redux, that goes:
//
// we have a State, we have an Action type, and we have a reducer function
// with type `(State, Action) -> State`
//
// reducers compose, not yet clear how that works in rust, but they do!
// maybe for the beginning, we have one shitty large Struct
//
// how does this help us? right now we do an awkward locking dance for the
// state struct; instead we can dispatch that to a single function that 
// only has the job to update the state, nothing else.
