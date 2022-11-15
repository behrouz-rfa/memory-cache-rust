use std::marker::PhantomData;
use std::ptr::NonNull;


pub struct KeyRef<K> {
    k: NonNull<K>,
}

pub struct LinkedList<K, V> {
    head: Option<NonNull<Node<K, V>>>,
    tail: Option<NonNull<Node<K, V>>>,
    len: usize,
    marker: PhantomData<Box<Node<K, V>>>,
}

struct Node<K, V> {
    next: Option<NonNull<Node<K, V>>>,
    prev: Option<NonNull<Node<K, V>>>,
    key: K,
    val: V,
}


impl<K, V> Node<K, V> {
    fn new(key: K, val: V) -> Self {
        Node { next: None, prev: None, key, val }
    }

    fn into_element(self: Box<Self>) -> V {
        self.val
    }
}

impl<K, V> Default for LinkedList<K, V> {
    /// Creates an empty `LinkedList<T>`.
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V: Eq> Eq for LinkedList<K, V> {}

impl<K, V: PartialEq> PartialEq for LinkedList<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.eq(other)
    }
}

impl<K, V: Eq> Eq for Node<K, V> {}

impl<K, V: PartialEq> PartialEq for Node<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.eq(other)
    }
}

// private methods
impl<K, V> LinkedList<K, V> {
    /// Adds the given node to the front of the list.
    #[inline]
    fn push_front_node(&mut self, mut node: Box<Node<K, V>>) {
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
    fn pop_front_node(&mut self) -> Option<Box<Node<K, V>>> {
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
}

impl<K, V> LinkedList<K, V> {
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        LinkedList { head: None, tail: None, len: 0, marker: PhantomData }
    }

    pub fn push_front(&mut self, key: K, val: V) {
        self.push_front_node(Box::new(Node::new(key, val)));
    }

     fn unlink_node(&mut self, mut node: NonNull<Node<K, V>>) {
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
    pub fn pop_front(&mut self) -> Option<V> {
        self.pop_front_node().map(Node::into_element)
    }
    pub fn move_to_front(&mut self, elt: K)
        where V: PartialEq,
              K: PartialEq
    {
        let mut current = self.head;
        while let Some(mut node) = current {
            unsafe {
                current = node.as_ref().next;
                if node.as_mut().key == elt {
                    self.unlink_node(node)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search() {
        let mut list = LinkedList::new();
        list.push_front("key", "val");
        list.push_front("key2", "val2");
        list.push_front("key3", "val3");


        println!("{:?}", list.pop_front());
        println!("{:?}", list.pop_front());
        println!("{:?}", list.pop_front());
    }
}