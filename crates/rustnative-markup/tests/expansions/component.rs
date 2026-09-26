fn expansion() -> ::rustnative_core::Node {
    {
        let __node = (context)
            .child_with_props::<
                Card,
                _,
            >(
                "card",
                {
                    type __Props = <Card as ::rustnative_core::Component>::Props;
                    #[allow(
                        clippy::needless_update,
                        reason = "every prop may be written"
                    )]
                    let __props = __Props {
                        title: ::core::convert::Into::into("Hi"),
                        ..::core::default::Default::default()
                    };
                    __props
                },
                <Card as ::rustnative_core::Component>::new,
            );
        let __node = (emphasized)(__node);
        __node
    }
}
