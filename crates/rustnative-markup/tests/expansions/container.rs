fn expansion() -> ::rustnative_core::Node {
    {
        let __layout = <::rustnative_core::LayoutStyle as ::core::default::Default>::default();
        let __container = <::rustnative_core::ColumnStyle as ::core::default::Default>::default()
            .gap(4);
        let __node = ::rustnative_core::Node::column_with_layout(
            "list",
            {
                let mut __children: ::std::vec::Vec<::rustnative_core::Node> = ::std::vec::Vec::new();
                if show {
                    __children
                        .push({
                            let __layout = <::rustnative_core::LayoutStyle as ::core::default::Default>::default();
                            let __node = ::rustnative_core::Node::label_with_layout(
                                "a",
                                "A",
                                __layout,
                            );
                            __node
                        });
                } else {
                    __children
                        .push({
                            let __layout = <::rustnative_core::LayoutStyle as ::core::default::Default>::default();
                            let __node = ::rustnative_core::Node::label_with_layout(
                                "b",
                                "B",
                                __layout,
                            );
                            __node
                        });
                }
                for item in items {
                    __children
                        .push({
                            let __layout = <::rustnative_core::LayoutStyle as ::core::default::Default>::default();
                            let __node = ::rustnative_core::Node::label_with_layout(
                                item,
                                item,
                                __layout,
                            );
                            __node
                        });
                }
                ::rustnative_core::IntoChildren::extend_into(extra, &mut __children);
                __children
            },
            __layout,
            __container,
        );
        __node
    }
}
