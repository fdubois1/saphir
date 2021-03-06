use http::*;
use utils::ToRegex;
use utils::RequestContinuation;
use regex::Regex;
use std::sync::RwLock;

/// Trait representing a controller
pub trait Controller: Send + Sync {
    /// Method invoked if the request gets routed to this controller. Nothing will be processed after a controller `handling` a request.
    /// When returning from this function, the `res` param is the response returned to the client.
    fn handle(&self, req: &SyncRequest, res: &mut SyncResponse);
}

///
pub struct RequestGuardCollection {
    guards: Vec<Box<RequestGuard>>
}

impl RequestGuardCollection {
    ///
    pub fn new() -> Self {
        RequestGuardCollection {
            guards: Vec::new(),
        }
    }

    ///
    pub fn add<G: 'static + RequestGuard>(&mut self, guard: G) {
        self.guards.push(Box::new(guard));
    }
}

impl<G: 'static + RequestGuard> From<G> for RequestGuardCollection {
    fn from(guard: G) -> Self {
        let mut reqg = RequestGuardCollection::new();
        reqg.add(guard);
        reqg
    }
}

impl<'a, G: 'static + RequestGuard + Clone> From<&'a [G]> for RequestGuardCollection {
    fn from(guards: &'a [G]) -> Self {
        let mut reqg = RequestGuardCollection::new();
        for guard in guards.to_vec() {
            reqg.add(guard);
        }
        reqg
    }
}

impl<G: 'static + RequestGuard> From<Vec<G>> for RequestGuardCollection {
    fn from(guards: Vec<G>) -> Self {
        let mut reqg = RequestGuardCollection::new();
        for guard in guards {
            reqg.add(guard);
        }
        reqg
    }
}

use ::std::slice::Iter;

impl<'a> IntoIterator for &'a RequestGuardCollection {
    type Item = &'a Box<RequestGuard>;
    type IntoIter = Iter<'a, Box<RequestGuard>>;

    fn into_iter(self) -> <Self as IntoIterator>::IntoIter {
        self.guards.iter()
    }
}

/// A trait to provide an other layer of validation before allowing a request into a controller
pub trait RequestGuard {
    ///
    fn validate(&self, req: &SyncRequest, res: &mut SyncResponse) -> RequestContinuation;
}

type DelegateFunction<T> = Fn(&T, &SyncRequest, &mut SyncResponse);
type ControllerDelegate<T> = (Method, Regex, Option<RequestGuardCollection>, Box<DelegateFunction<T>>);

/// Struct to delegate a request to a registered function matching booth a `method` and a `path`
pub struct ControllerDispatch<T> {
    /// The context sent with the request to the function
    delegate_context: T,
    /// List of delegates
    delegates: RwLock<Vec<ControllerDelegate<T>>>,
}

impl<T: Send + Sync> ControllerDispatch<T> {
    ///
    pub fn new(delegate_context: T) -> Self {
        ControllerDispatch {
            delegate_context,
            delegates: RwLock::new(Vec::new()),
        }
    }

    /// Add a delegate function to handle a particular request
    /// # Example
    ///
    /// ```rust,no_run
    /// let u8_context = 1;
    /// let dispatch = ControllerDispatch::new(u8_context);
    /// dispatch.add(Method::Get, "^/test$", |ctx, req, res| { println!("this will handle Get request done on <your_host>/test")});
    /// ```
    pub fn add<F, R: ToRegex>(&self, method: Method, path: R, delegate_func: F)
        where for<'r, 's, 't0> F: 'static + Fn(&'r T, &'s SyncRequest, &'t0 mut SyncResponse) {
        self.delegates.write().unwrap().push((method, reg!(path), None, Box::new(delegate_func)));
    }

    /// Add a delegate function to handle a particular request
    /// # Example
    ///
    /// ```rust,no_run
    /// let u8_context = 1;
    /// let guard = BodyGuard;
    /// let dispatch = ControllerDispatch::new(u8_context);
    /// dispatch.add_with_guards(Method::Get, "^/test$", guard.into(), |ctx, req, res| { println!("this will handle Get request done on <your_host>/test")});
    /// ```
    pub fn add_with_guards<F, R: ToRegex>(&self, method: Method, path: R, guards: RequestGuardCollection, delegate_func: F)
        where for<'r, 's, 't0> F: 'static + Fn(&'r T, &'s SyncRequest, &'t0 mut SyncResponse) {
        self.delegates.write().unwrap().push((method, reg!(path), Some(guards), Box::new(delegate_func)));
    }

    ///
    pub fn dispatch(&self, req: &SyncRequest, res: &mut SyncResponse) {
        use std::iter::FromIterator;
        let delegates_list = self.delegates.read().unwrap();
        let method = req.method().clone();

        let retained_delegate = Vec::from_iter(delegates_list.iter().filter(move |x| {
            x.0 == method
        }));

        if retained_delegate.len() == 0 {
            res.status(StatusCode::METHOD_NOT_ALLOWED);
            return;
        }

        for del in retained_delegate {
            let (_, ref reg, ref op_guards, ref boxed_func) = del;

            if reg.is_match(req.uri().path()) {
                if let Some(ref guards) = op_guards {
                    for guard in guards {
                        if let RequestContinuation::None = guard.validate(req, res) {
                            return;
                        }
                    }
                }
                boxed_func(&self.delegate_context, req, res);
                return;
            }
        }

        res.status(StatusCode::BAD_REQUEST);
    }
}

unsafe impl<T> Sync for ControllerDispatch<T> {}

unsafe impl<T> Send for ControllerDispatch<T> {}

/// An helper struct embedding a `ControllerDispatch`.
pub struct BasicController<C> {
    dispatch: ControllerDispatch<C>
}

impl<C: Send + Sync> Controller for BasicController<C> {
    fn handle(&self, req: &SyncRequest, res: &mut SyncResponse) {
        self.dispatch.dispatch(req, res);
    }
}

impl<C: Send + Sync> BasicController<C> {
    ///
    pub fn new(controller_context: C) -> Self {
        BasicController {
            dispatch: ControllerDispatch::new(controller_context),
        }
    }

    /// Add a delegate function to handle a particular request
    /// # Example
    ///
    /// ```rust,no_run
    /// let u8_context = 1;
    /// let u8_controller = BasicController::new(u8_context);
    /// u8_controller.add(Method::Get, "^/test$", |ctx, req, res| { println!("this will handle Get request done on <your_host>/test")});
    /// ```
    pub fn add<F, R: ToRegex>(&self, method: Method, path: R, delegate_func: F)
        where for<'r, 's, 't0> F: 'static + Fn(&'r C, &'s SyncRequest, &'t0 mut SyncResponse) {
        self.dispatch.add(method, path, delegate_func);
    }

    /// Add a delegate function to handle a particular request
    /// # Example
    ///
    /// ```rust,no_run
    /// let u8_context = 1;
    /// let u8_controller = BasicController::new(u8_context);
    /// u8_controller.add(Method::Get, "^/test$", |ctx, req, res| { println!("this will handle Get request done on <your_host>/test")});
    /// ```
    pub fn add_with_guards<F, R: ToRegex>(&self, method: Method, path: R, guards: RequestGuardCollection, delegate_func: F)
        where for<'r, 's, 't0> F: 'static + Fn(&'r C, &'s SyncRequest, &'t0 mut SyncResponse) {
        self.dispatch.add_with_guards(method, path, guards, delegate_func);
    }
}

/// RequestGuard ensuring that a request has a body
pub struct BodyGuard;

impl RequestGuard for BodyGuard {
    fn validate(&self, req: &SyncRequest, _res: &mut SyncResponse) -> RequestContinuation {
        if req.body().len() <= 0 {
            return RequestContinuation::None
        }

        RequestContinuation::Next
    }
}

