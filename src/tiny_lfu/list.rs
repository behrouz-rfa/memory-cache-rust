use std::marker::PhantomData;
use std::ptr::NonNull;


pub struct LinkedList<V> {
    head: Option<NonNull<Node<V>>>,
    tail: Option<NonNull<Node<V>>>,
    len: usize,
    marker: PhantomData<Box<Node<V>>>,
}

pub struct Node<V> {
    pub(crate) next: Option<NonNull<Node<V>>>,
    pub(crate)  prev: Option<NonNull<Node<V>>>,

    pub(crate) element: V,
}


impl<V> Node<V> {
    fn new(val: V) -> Self {
        Node { next: None, prev: None, element: val }
    }

    fn into_element(self: Box<Self>) -> V {
        self.element
    }
}

impl<V:Clone> Default for LinkedList<V> {
    /// Creates an empty `LinkedList<T>`.
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<V: Eq> Eq for LinkedList<V> {}

impl<V: PartialEq> PartialEq for LinkedList<V> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.eq(other)
    }
}

impl<V: Eq> Eq for Node<V> {}

impl<V: PartialEq> PartialEq for Node<V> {
    fn eq(&self, other: &Self) -> bool {
        self.eq(other)
    }
}

// private methods
impl<V: Clone> LinkedList<V> {
    /// Adds the given node to the front of the list.
    #[inline]
    fn push_front_node(&mut self, mut node: Box<Node<V>>) {
        // This method takes care not to create mutable references to whole nodes,
        // to maintain validity of aliasing pointers into `element`.
        unsafe {
            node.next = self.head;
            node.prev = None;
            let node = Some(Box::leak(node).into());

            match self.head {
                None => self.tail = node,
                // Not creating new mutable (unique!) references overlapping `element`.
                Some(head) => (*head.as_ptr()).prev = node,
            }

            self.head = node;
            self.len += 1;
        }
    }

    #[inline]
    fn pop_front_node(&mut self) -> Option<Box<Node<V>>> {
        // This method takes care not to create mutable references to whole nodes,
        // to maintain validity of aliasing pointers into `element`.
        self.head.map(|node| unsafe {
            let node = Box::from_raw(node.as_ptr());
            self.head = node.next;

            match self.head {
                None => self.tail = None,
                // Not creating new mutable (unique!) references overlapping `element`.
                Some(head) => (*head.as_ptr()).prev = None,
            }

            self.len -= 1;
            node
        })
    }


    /// Adds the given node to the back of the list.
    #[inline]
    fn push_back_node(&mut self, mut node: Box<Node<V>>) {
        // This method takes care not to create mutable references to whole nodes,
        // to maintain validity of aliasing pointers into `element`.
        unsafe {
            node.next = None;
            node.prev = self.tail;
            let node = Some(Box::leak(node).into());

            match self.tail {
                None => self.head = node,
                // Not creating new mutable (unique!) references overlapping `element`.
                Some(tail) => (*tail.as_ptr()).next = node,
            }

            self.tail = node;
            self.len += 1;
        }
    }

    /// Removes and returns the node at the back of the list.
    #[inline]
    fn pop_back_node(&mut self) -> Option<Box<Node<V>>> {
        // This method takes care not to create mutable references to whole nodes,
        // to maintain validity of aliasing pointers into `element`.
        self.tail.map(|node| unsafe {
            let node = Box::from_raw(node.as_ptr());
            self.tail = node.prev;

            match self.tail {
                None => self.head = None,
                // Not creating new mutable (unique!) references overlapping `element`.
                Some(tail) => (*tail.as_ptr()).next = None,
            }

            self.len -= 1;
            node
        })
    }
}

impl<V: Clone> LinkedList<V> {
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        LinkedList { head: None, tail: None, len: 0, marker: PhantomData }
    }

    pub fn len(&self)->usize {
        self.len
    }

    pub fn back(&mut self) -> Option<V> {
        unsafe { self.tail.as_mut().map(|node|  (*node.as_ptr()).element.clone()) }
    }

    pub fn push_front(&mut self, val: V) {
        self.push_front_node(Box::new(Node::new(val)));
    }
    pub fn push_back(&mut self, val: V) {
        self.push_back_node(Box::new(Node::new(val)));
    }
    fn unlink_node(&mut self, mut node: NonNull<Node<V>>) {
        let node = unsafe { node.as_mut() };
        match node.prev {
            Some(prev) => unsafe { (*prev.as_ptr()).next = node.next }
            Node => self.head = node.next
        }

        match node.next {
            Some(next) => unsafe { (*next.as_ptr()).prev = node.prev }
            None => self.tail = node.prev,
        }


        node.next = self.head;
        node.prev = None;

        let node = Some(NonNull::from(node));
        match self.head {
            None => self.tail = node,
            // Not creating new mutable (unique!) references overlapping `element`.
            Some(head) => unsafe { (*head.as_ptr()).prev = node },
        }

        self.head = node;


        // self.len -= 1;
    }
    fn remove_unlink(&mut self, mut node: NonNull<Node<V>>) {
        let node = unsafe { node.as_mut() }; // this one is ours now, we can create an &mut.

        // Not creating new mutable (unique!) references overlapping `element`.
        match node.prev {
            Some(prev) => unsafe { (*prev.as_ptr()).next = node.next },
            // this node is the head node
            None => self.head = node.next,
        };

        match node.next {
            Some(next) => unsafe { (*next.as_ptr()).prev = node.prev },
            // this node is the tail node
            None => self.tail = node.prev,
        };

        self.len -= 1;

        // self.len -= 1;
    }
    pub fn pop_front(&mut self) -> Option<V> {
        self.pop_front_node().map(Node::into_element)
    }

    pub fn pop_back(&mut self) -> Option<V> {
        self.pop_back_node().map(Node::into_element)
    }
    pub fn move_to_front(&mut self, elt: V)
        where V: PartialEq,
    {
        let mut current = self.head;
        while let Some(mut node) = current {
            unsafe {
                current = node.as_ref().next;
                if node.as_mut().element == elt {
                    self.unlink_node(node)
                }
            }
        }
    }

    pub fn remove(&mut self, elt: V)
        where V: PartialEq,
    {
        let mut current = self.head;
        while let Some(mut node) = current {
            unsafe {
                current = node.as_ref().next;
                if node.as_mut().element == elt {
                    self.remove_unlink(node)
                }
            }
        }
    }

    pub fn contains(&mut self, elt: V) -> bool
        where V: PartialEq,
    {
        let mut current = self.head;
        while let Some(mut node) = current {
            unsafe {
                current = node.as_ref().next;
                if node.as_mut().element == elt {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_front() {
        let mut list = LinkedList::new();
        list.push_front("key");
        list.push_front("key2");
        list.push_front("key3");


        assert_eq!(list.pop_front(), Some("key3"));
        assert_eq!(list.pop_front(), Some("key2"));
        assert_eq!(list.pop_front(), Some("key"));
        assert_eq!(list.pop_front(), None);
    }

    #[test]
    fn test_move_to_front() {
        let mut list = LinkedList::new();
        list.push_front("key");
        list.push_front("key2");
        list.push_front("key3");
        list.move_to_front("key3");


        assert_eq!(list.pop_front(), Some("key3"));

    }
    #[test]
    fn test_push_back() {
        let mut list = LinkedList::new();
        list.push_back("key");
        list.push_back("key2");
        list.push_back("key3");


        assert_eq!(list.pop_back(), Some("key3"));
        assert_eq!(list.pop_back(), Some("key2"));
        assert_eq!(list.pop_back(), Some("key"));
        assert_eq!(list.pop_back(), None);
    }
    #[test]
    fn test_remove() {
        let mut list = LinkedList::new();
        list.push_back("key");
        list.push_back("key2");
        list.push_back("key3");


        list.remove("key2");

        assert_eq!(list.contains("key2"), false);
    }
}