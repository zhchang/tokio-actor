extern crate proc_macro;

use std::collections::HashMap;

use inflector::Inflector;
use proc_macro::TokenStream;
use quote::__private::Span;
use quote::quote;
use syn::parse::{Parse, Parser};
use syn::{parse_macro_input, Ident, ItemEnum, ItemImpl, ItemMod, ItemStruct, Type};

struct ActorInfo {
    actor_ident: Option<Ident>,
    msg_ident: Ident,
    msg_mapping: HashMap<Ident, Type>,
}

impl ActorInfo {
    fn new(msg_ident: Ident) -> Self {
        Self {
            actor_ident: None,
            msg_ident,
            msg_mapping: HashMap::new(),
        }
    }
}

enum ID {
    RemoveMsg(Ident),
    Direct(Ident),
}

fn get_actor_name(id: ID) -> Option<String> {
    match id {
        ID::RemoveMsg(v) => {
            let name = format!("{}", v);
            if name.ends_with("Msg") {
                return Some(name[..(name.len() - 3)].to_string());
            }
            None
        }
        ID::Direct(v) => {
            let name = format!("{}", v);
            Some(name)
        }
    }
}

fn process_enum(item: &mut ItemEnum, info: &mut ActorInfo) {
    for v in &mut item.variants {
        match &mut v.fields {
            syn::Fields::Named(fields) => {
                let mut new_list = vec![];
                for field in &mut fields.named {
                    if field.ident.is_some() && field.ident.as_ref().unwrap() == "resp" {
                        let ty = field.ty.clone();
                        info.msg_mapping.insert(v.ident.clone(), ty.clone());
                        new_list.push(
                            syn::Field::parse_named
                                .parse2(quote! { resp: Option<tokio::sync::oneshot::Sender<#ty>>})
                                .unwrap(),
                        );
                    } else {
                        new_list.push(field.clone());
                    }
                }
                fields.named.clear();
                for v in new_list {
                    fields.named.push(v);
                }
            }
            _ => {}
        }
    }
}
fn process_struct(item: &mut ItemStruct, info: &mut ActorInfo) {
    match &mut item.fields {
        syn::Fields::Named(fields) => {
            let msg_type = info.msg_ident.clone();
            fields.named.push(
                syn::Field::parse_named
                    .parse2(quote! { receiver: tokio::sync::mpsc::UnboundedReceiver<#msg_type>})
                    .unwrap(),
            );
        }
        _ => {}
    }
}

#[proc_macro_attribute]
pub fn actors(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut ast = parse_macro_input!(item as ItemMod);
    let mut context = HashMap::<String, ActorInfo>::new();
    if let Some(content) = &mut ast.content {
        for item in &mut content.1 {
            match item {
                syn::Item::Enum(v) => {
                    let actor_name = get_actor_name(ID::RemoveMsg(v.ident.clone()));
                    if let Some(name) = actor_name {
                        if !context.contains_key(&name) {
                            context.insert(name.clone(), ActorInfo::new(v.ident.clone()));
                        }
                        let info = context.get_mut(&name).unwrap();
                        process_enum(v, info)
                    }
                }
                _ => {}
            }
        }
        //println!("finished enum processing");
        let mut to_add = vec![];
        for item in &mut content.1 {
            match item {
                syn::Item::Struct(v) => {
                    let actor_name = get_actor_name(ID::Direct(v.ident.clone()));
                    if let Some(name) = actor_name {
                        if !context.contains_key(&name) {
                            continue;
                        }
                        let info = context.get_mut(&name).unwrap();
                        if info.msg_mapping.len() == 0 {
                            continue;
                        }
                        info.actor_ident = Some(v.ident.clone());
                        process_struct(v, info);
                        let actor_ident =
                            Ident::new(&format!("Actor{}", &v.ident), Span::call_site());
                        let msg_ident = info.msg_ident.clone();
                        to_add.push(quote! {
                            pub struct #actor_ident{
                                sender: tokio::sync::mpsc::UnboundedSender<#msg_ident>,
                            }
                        });
                    }
                }
                _ => {}
            }
        }
        for add in to_add {
            content
                .1
                .push(syn::Item::Struct(ItemStruct::parse.parse2(add).unwrap()));
        }
        //println!("finished struct processing");
        for (_name, info) in context.into_iter() {
            if info.msg_mapping.len() == 0 || info.actor_ident.is_none() {
                continue;
            }
            let ident = info.actor_ident.as_ref().unwrap().clone();
            let actor_ident = Ident::new(&format!("Actor{}", &ident), Span::call_site());
            let msg_ident = info.msg_ident.clone();
            let actor_impl = quote! {
                impl #actor_ident{
                    pub async fn new()->Self{
                        let (s, r) = tokio::sync::mpsc::unbounded_channel();
                        let mut a = #ident::new(r);
                        tokio::spawn(async move {
                            a.run().await;
                        });
                        return Self{sender:s};
                    }

                }
            };
            content
                .1
                .push(syn::Item::Impl(ItemImpl::parse.parse2(actor_impl).unwrap()));
            let o_impl = quote! {
                impl #ident{
                    fn new(r: tokio::sync::mpsc::UnboundedReceiver<#msg_ident>)->Self{
                        return Self{ receiver: r };
                    }

                    async fn run(&mut self){
                        while let Some(msg) = self.receiver.recv().await {
                            self.process(msg).await;
                        }
                    }
                }
            };
            content
                .1
                .push(syn::Item::Impl(ItemImpl::parse.parse2(o_impl).unwrap()));
            for (req, resp) in info.msg_mapping.into_iter() {
                let fname_wait =
                    Ident::new(&format!("{}", &req).to_snake_case(), Span::call_site());
                let method = quote! {
                    impl #actor_ident{
                        pub async fn #fname_wait(&mut self,mut msg:#msg_ident)->Result<#resp,&'static str>{
                            match msg{
                                #msg_ident::#req{ref mut resp,..}=>{
                                    let (mut s,mut r) = tokio::sync::oneshot::channel();
                                    *resp = Some(s);
                                    self.sender.send(msg).map_err(|_e|{return "send failed";})?;
                                    match r.await{
                                        Ok(v)=>{return Ok(v);}
                                        _=>{return Err("mailbox closed");}
                                    };
                                }
                                _=>{return Err("invalid msg type");}
                            };
                        }
                    }
                };
                content
                    .1
                    .push(syn::Item::Impl(ItemImpl::parse.parse2(method).unwrap()));
                let fname_nowait = Ident::new(
                    &format!("{}_no_wait", &req).to_snake_case(),
                    Span::call_site(),
                );
                let method_no_wait = quote! {
                    impl #actor_ident{
                        pub async fn #fname_nowait(&mut self,mut msg:#msg_ident)->Result<(),&'static str>{
                            match msg{
                                #msg_ident::#req{..}=>{
                                    self.sender.send(msg).map_err(|_e|{return "send failed";})?;
                                    return Ok(());
                                }
                                _=>{return Err("invalid msg type");}
                            };
                        }
                    }
                };
                content.1.push(syn::Item::Impl(
                    ItemImpl::parse.parse2(method_no_wait).unwrap(),
                ));
            }
        }
    }
    let result = quote! {#ast};
    //println!("{}", &result);
    return result.into();
}
