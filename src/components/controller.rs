use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use patternfly_yew::prelude::*;
use yew::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web3::{
    transports::{eip_1193::Eip1193, Either, Http},
    types::Address,
    Web3,
};

use crate::evm::{
    contracts::ERC1155Contract,
    types::{Bid, Field, Metadata}
};


#[derive(Properties)]
pub struct ControllerProps {
    pub handle_error: Callback<eyre::Report>,
    pub transport: Either<Eip1193, Http>,
    pub address: Option<Address>,
    pub mandelbrot: Arc<Mutex<mandelbrot_explorer::Interface>>,
    #[prop_or(1)]
    pub token_id: u128,
}

impl PartialEq for ControllerProps {
    fn eq(&self, other: &Self) -> bool {
        self.address == other.address && self.token_id == other.token_id
    }
}

#[derive(Clone)]
pub struct Controller {
    redraw: Callback<()>,
    address: Arc<Mutex<Option<Address>>>,
    mandelbrot: Arc<Mutex<mandelbrot_explorer::Interface>>,
    erc1155_contract: ERC1155Contract,
    nav_history: Arc<Mutex<Vec<Metadata>>>,
    children: Arc<Mutex<HashMap<u128, Metadata>>>,
    bids: Arc<Mutex<HashMap<u128, Bid>>>,
    bid_amount: Arc<Mutex<f64>>,
    bids_minimum_price: Arc<Mutex<f64>>,
    approve_amount_node_ref: NodeRef,
}

impl Controller {
    fn view_nft(&self, token_id: u128) {
        let this = self.clone();
        spawn_local(async move {
            if let Ok(metadata) = this.erc1155_contract.get_ancestry_metadata(token_id).await {
                let nav_history = &mut *this.nav_history.lock().unwrap();
                nav_history.clear();
                nav_history.extend(metadata.into_iter().rev());
                if let Some(token) = nav_history.last() {
                    this.mandelbrot.lock().unwrap().sample_location.move_into_frame(&token.to_frame(mandelbrot_explorer::FrameColor::Blue));
                }
            } else {
                this.view_nft(1);
            }
        });
        self.obtain_tokens(token_id);
    }

    fn obtain_tokens(&self, parent_id: u128) {
        spawn_local({
            let this = self.clone();
            async move {
                if let Ok(metadata) = this.erc1155_contract.get_children_metadata(parent_id).await {
                    let children = &mut (*this.children.lock().unwrap());
                    children.clear();
                    children.extend(metadata.into_iter().map(|m| (m.token_id, m)));
                }
                if let Ok(bids) = this.erc1155_contract.get_bids(parent_id).await {
                    let bids_ = &mut (*this.bids.lock().unwrap());
                    bids_.clear();
                    bids_.extend(bids.into_iter().map(|bid| (bid.bid_id, bid)));
                }
                this.check_ownership();
                this.update_frames();
                this.redraw.emit(());
            }
        });
    }

    fn check_ownership(&self) {
        if let Some(address) = *self.address.lock().unwrap() {
            self.children.lock().unwrap().values_mut().for_each(|token| {
                token.owned = token.owner == address;
            });
            self.bids.lock().unwrap().values_mut().for_each(|bid| {
                bid.owned = bid.recipient == address;
            });
            self.nav_history.lock().unwrap().iter_mut().for_each(|token| {
                token.owned = token.owner == address;
            });
        }
    }

    fn update_frames(&self) {
        let mandelbrot = &mut self.mandelbrot.lock().unwrap();
        let frames = &mut mandelbrot.frames;
        frames.clear();
        frames.extend(self.children.lock().unwrap().values().map(|token| token.to_frame(mandelbrot_explorer::FrameColor::Red)));
        frames.extend(self.bids.lock().unwrap().values().map(|token| token.to_frame()));
        frames.extend(self.nav_history.lock().unwrap().iter().rev().map(|token| token.to_frame(mandelbrot_explorer::FrameColor::Blue)));
        if let Some(redraw) = &mandelbrot.redraw {
            redraw();
        }
    }
}

impl Component for Controller {
    type Message = ();
    type Properties = ControllerProps;

    fn create(ctx: &Context<Self>) -> Self {
        let mandelbrot = ctx.props().mandelbrot.clone();
        let token_id = ctx.props().token_id;
        let transport = ctx.props().transport.clone();
        let web3 = Web3::new(transport);

        let this = Self {
            redraw: ctx.link().callback(|_| ()),
            address: Arc::new(Mutex::new(None)),
            mandelbrot: mandelbrot.clone(),
            erc1155_contract: ERC1155Contract::new(&web3, ctx.props().handle_error.clone()),
            nav_history: Arc::new(Mutex::new(Vec::new())),
            children: Arc::new(Mutex::new(HashMap::new())),
            bids: Arc::new(Mutex::new(HashMap::new())),
            bid_amount: Arc::new(Mutex::new(0.0)),
            bids_minimum_price: Arc::new(Mutex::new(0.0)),
            approve_amount_node_ref: NodeRef::default(),
        };

        let on_frame_selected = Callback::from({
            let this = this.clone();
            move |frame: mandelbrot_explorer::Frame| {
                match frame.color {
                    mandelbrot_explorer::FrameColor::Red |
                    mandelbrot_explorer::FrameColor::Pink |
                    mandelbrot_explorer::FrameColor::Blue |
                    mandelbrot_explorer::FrameColor::LightBlue => {
                        this.mandelbrot.lock().unwrap().sample_location.move_into_frame(&frame);
                        let nav_history = &mut this.nav_history.lock().unwrap();
                        for (token_id, token) in this.children.lock().unwrap().iter() {
                            if *token_id == frame.id {
                                nav_history.push(token.clone());
                                break
                            }
                        }
                        for (i, token) in nav_history.iter().enumerate().rev() {
                            if token.token_id == frame.id {
                                nav_history.truncate(i + 1);
                                break
                            }
                        }

                        if let Some(node) = this.approve_amount_node_ref.get() {
                            node.set_text_content(Some(&"0".to_string()));
                        }
    
                        this.obtain_tokens(frame.id);
                    }
                    mandelbrot_explorer::FrameColor::Yellow |
                    mandelbrot_explorer::FrameColor::Lemon => {
                        if let Some(bid) = this.bids.lock().unwrap().get_mut(&frame.id) {
                            bid.selected = true;
                        }
                        this.update_frames();
                        this.redraw.emit(());
                    }
                    mandelbrot_explorer::FrameColor::Green => {
                        if let Some(bid) = this.bids.lock().unwrap().get_mut(&frame.id) {
                            bid.selected = false;
                        }
                        this.update_frames();
                        this.redraw.emit(());
                    }
                }
            }
        });

        mandelbrot.lock().unwrap().frame_selected_callback = Some(Arc::new({
            let on_frame_selected = on_frame_selected.clone();
            move |frame| on_frame_selected.emit(frame.clone())
        }));

        this.view_nft(token_id);
        this
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let address = ctx.props().address.clone();
        *self.address.lock().unwrap() = if let Some(address) = address.clone() {
            Some(address.clone())
        } else {
            None
        };
        self.check_ownership();
        self.update_frames();
        if let Some(redraw) = &self.mandelbrot.lock().unwrap().redraw {
            redraw();
        }

        let on_burn_clicked = {
            let this = self.clone();
            let address = address.clone();
            move |token_id| {
                let this = this.clone();
                if let Some(address) = address {
                    spawn_local(async move {
                        this.erc1155_contract.burn(address, token_id).await;
                    });
                }
            }
        };

        let change_bid_amount = {
            let bid_amount = self.bid_amount.clone();
            move |value: String| {
                if let Ok(value) = value.parse::<f64>() {
                    *bid_amount.lock().unwrap() = value;
                }
            }
        };

        let change_bids_minimum_price = {
            let bids_minimum_price = self.bids_minimum_price.clone();
            move |value: String| {
                if let Ok(value) = value.parse::<f64>() {
                    *bids_minimum_price.lock().unwrap() = value;
                }
            }
        };

        let on_bid_clicked = {
            let this = self.clone();
            let address = address.clone();
            move |_| {
                let this = this.clone();
                if let Some(address) = address {
                    let params = this.mandelbrot.lock().unwrap().sample_location.to_mandlebrot_params(0);
                    spawn_local(async move {
                        if let Some(token) = this.nav_history.lock().unwrap().last() {
                            this.erc1155_contract.bid(
                                address,
                                token.token_id,
                                Field {
                                    x_min: params.x_min as f64,
                                    y_min: params.y_min as f64,
                                    x_max: params.x_max as f64,
                                    y_max: params.y_max as f64
                                },
                                *this.bid_amount.lock().unwrap(),
                                *this.bids_minimum_price.lock().unwrap(),
                            ).await;
                        }
                    });
                }
            }
        };

        // let on_mint_clicked = {
        //     let this = self.clone();
        //     let ethereum = ethereum.clone();
        //     move |_| {
        //         let this = this.clone();
        //         if let Some(address) = ethereum.address() {
        //             let address = address.clone();
        //             let params = this.mandelbrot.lock().unwrap().sample_location.to_mandlebrot_params(0);
        //             spawn_local(async move {
        //                 this.erc1155_contract.mint(
        //                     address,
        //                     *this.selected_nft_id.lock().unwrap(),
        //                     Field {
        //                         x_min: params.x_min as f64,
        //                         y_min: params.y_min as f64,
        //                         x_max: params.x_max as f64,
        //                         y_max: params.y_max as f64
        //                     }
        //                 ).await;
        //             });
        //         }
        //     }
        // };

        let on_bid_toggled = {
            let this = self.clone();
            move |bid_id, state| {
                {
                    let mut bids_lock = this.bids.lock().unwrap();
                    if let Some(bid) = bids_lock.get_mut(&bid_id) {
                        bid.selected = state;

                        let total_approve_amount: f64 = bids_lock.values()
                            .filter(|bid| bid.selected)
                            .map(|bid| bid.amount)
                            .sum();
                        this.approve_amount_node_ref.get().unwrap().set_text_content(Some(&total_approve_amount.to_string()));
                    }
                }
                this.update_frames();
            }
        };

        let on_approve_clicked = {
            let this = self.clone();
            let address = address.clone();
            move |_| {
                let this = this.clone();
                if let Some(address) = address {
                    spawn_local(async move {
                        let selected_bids: Vec<u128> = this.bids.lock().unwrap()
                            .values()
                            .filter(|bid| bid.selected)
                            .map(|bid| bid.bid_id)
                            .collect();
                        this.erc1155_contract.batch_approve_bids(address, &selected_bids).await;
                    });
                }
            }
        };

        let on_delete_clicked = {
            let this = self.clone();
            let address = address.clone();
            move |bid_id| {
                let this = this.clone();
                if let Some(address) = address {
                    spawn_local(async move {
                        this.erc1155_contract.delete_bid(address, bid_id).await;
                    });
                }
            }
        };

        let (
            token_id,
            owner,
            locked_fuel,
            minimum_price
        ) = if let Some(token) = self.nav_history.lock().unwrap().last() {
            (token.token_id, token.owner.to_string(), token.locked_fuel.to_string(), token.minimum_price.to_string())
        } else {
            (0, "".to_string(), 0.to_string(), 0.to_string())
        };

        let bids_lock = self.bids.lock().unwrap();
        let mut bids: Vec<&Bid> = bids_lock.values().collect();
        bids.sort_by(|bid_a, bid_b| bid_a.amount.partial_cmp(&bid_b.amount).unwrap());
        let total_approve_amount: f64 = bids.iter().filter(|bid| bid.selected).map(|bid| bid.amount).sum();

        html! {
            <div>
                <Stack>
                    <StackItem>
                        <p><label>{format!("NFT id: {}", token_id)}</label></p>
                        <p><label>{format!("Owner: {}", owner)}</label></p>
                        <p><label>{format!("Locked FUEL: {}", locked_fuel)}</label></p>
                        <p><label>{format!("Minimum bid: {}", minimum_price)}</label></p>
                        if address.is_some() {
                            <p><button onclick={move |_| on_burn_clicked(token_id)}>{ "Burn" }</button></p>
                            <TextInputGroup>
                                <p>
                                    <TextInputGroupMain
                                        placeholder="Bid amount"
                                        r#type="number"
                                        oninput={change_bid_amount}
                                    />
                                    <TextInputGroupMain
                                        placeholder="Minimum bid price"
                                        r#type="number"
                                        oninput={change_bids_minimum_price}
                                    />
                                </p>
                                <TextInputGroupUtilities>
                                    <Button
                                        label="Bid"
                                        variant={ButtonVariant::Primary}
                                        onclick={on_bid_clicked}
                                    />
                                </TextInputGroupUtilities>
                            </TextInputGroup>
                        }
                    </StackItem>
                    if address.is_some() {
                        // <StackItem>
                        //     <button onclick={on_mint_clicked}>{ "Mint" }</button>
                        // </StackItem>
                        if bids.len() > 0 {
                            <StackItem>
                                <br/>
                                <p>{ "Bids:" }</p>
                                {
                                    for bids.iter().map(|bid| {
                                        let on_bid_toggled = on_bid_toggled.clone();
                                        let on_delete_clicked = on_delete_clicked.clone();
                                        let bid_id = bid.bid_id;
                                        html_nested!{
                                            <p>
                                                <Switch
                                                    label={format!("{} {:?}", bid.amount.to_string(), bid.recipient)}
                                                    checked={bid.selected}
                                                    onchange={move |state| on_bid_toggled(bid_id, state)}
                                                />
                                                <button onclick={move |_| on_delete_clicked(bid_id)}>{ "Delete" }</button>
                                            </p>
                                        }
                                    })
                                }
                                <p>
                                    <label ref={self.approve_amount_node_ref.clone()}>{ total_approve_amount }</label>
                                    <button onclick={on_approve_clicked}>{ "Approve" }</button>
                                </p>
                            </StackItem>
                        }
                    }
                </Stack>
            </div>
        }
    }
}
