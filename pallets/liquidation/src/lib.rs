#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

use frame_support::{
	dispatch::DispatchResult,
	storage::types::StorageMap,
	traits::{Currency, ExistenceRequirement, Get},
	weights::WeightToFeePolynomial,
	Twox64Concat,
};
use frame_system::{pallet_prelude::{BlockNumberFor, OriginFor}, ensure_signed_or_root};
use mp_system::BASE_ACCOUNT;
pub use pallet::*;
use sp_runtime::{
	traits::{StaticLookup, Zero},
	AccountId32, Perbill, Saturating,
};
use sp_std::{prelude::*, sync::Arc, vec};

use xcm::{
	opaque::v4::Junctions::X1,
	prelude::*,
	v4::{Asset, AssetId, Junction, Location, NetworkId},
	VersionedAssets, VersionedLocation,
};

type BalanceOf<T> =
	<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

pub type Balance = u128;

pub const PARACHAIN_TO_RELAYCHAIN_UNIT: u128 = 1_000_000;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;
	use pallet_balances;
	use pallet_order::OrderGasCost;
	use pallet_utility;

	#[pallet::pallet]
	#[pallet::without_storage_info]
	pub struct Pallet<T>(_);

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config:
		frame_system::Config
		+ pallet_order::Config
		+ pallet_pot::Config
		+ pallet_balances::Config
		+ pallet_utility::Config
	{
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

		///handle transfer
		type Currency: frame_support::traits::Currency<Self::AccountId>
			+ frame_support::traits::ReservableCurrency<Self::AccountId>;

		/// The XCM sender
		type XcmSender: SendXcm;

		//Treasury account on the Relay Chain
		//#[pallet::constant]
		//type RelayChainTreasuryAccountId: Get<AccountId32>;

		///Handles converting weight to fee value
		type WeightToFee: WeightToFeePolynomial<Balance = Balance>;

		///get real weight cost from coreTime placeOrder pallet
		type OrderGasCost: OrderGasCost<Self>;

		/// ED necessitate the account to exist
		#[pallet::constant]
		type ExistentialDeposit: Get<Balance>;

		/// system accountId
		#[pallet::constant]
		type SystemAccountName: Get<&'static str>;

		/// treasury accountId
		#[pallet::constant]
		type TreasuryAccountName: Get<&'static str>;

		/// operation accountId
		#[pallet::constant]
		type OperationAccountName: Get<&'static str>;
	}

	#[pallet::storage]
	#[pallet::getter(fn base_account_balance)]
	pub type BaseAccountReserved<T: Config> = StorageValue<_, Balance, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn collator_real_gas_costs)]
	pub type CollatorRealGasCosts<T: Config> =
		StorageMap<_, Twox64Concat, T::AccountId, Balance, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn total_income)]
	pub type TotalIncome<T: Config> = StorageValue<_, Balance, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn total_cost)]
	pub type TotalCost<T: Config> = StorageValue<_, Balance, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn block_count)]
	pub type DistributionBlockCount<T: Config> = StorageValue<_, BlockNumberFor<T>, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn system_ratio)]
	pub type SystemRatio<T: Config> = StorageValue<_, Perbill, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn treasury_ratio)]
	pub type TreasuryRatio<T: Config> = StorageValue<_, Perbill, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn operation_ratio)]
	pub type OperationRatio<T: Config> = StorageValue<_, Perbill, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn min_liquidation_threshold)]
	pub type MinLiquidationThreshold<T: Config> = StorageValue<_, Balance, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn profit_distribution_cycle)]
	pub type ProfitDistributionCycle<T: Config> = StorageValue<_, BlockNumberFor<T>, ValueQuery>;

	/// The pallet admin key.
	#[pallet::storage]
	#[pallet::getter(fn admin_key)]
	pub type Admin<T: Config> = StorageValue<_, T::AccountId>;

	#[pallet::genesis_config]
	#[derive(frame_support::DefaultNoBound)]
	pub struct GenesisConfig<T: Config> {
		/// The `AccountId` of the admin key.
		pub admin_key: Option<T::AccountId>,
		pub system_ratio: Perbill,
		pub treasury_ratio: Perbill,
		pub operation_ratio: Perbill,
		pub min_liquidation_threshold: Balance,
		pub profit_distribution_cycle: BlockNumberFor<T>,
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
		fn build(&self) {
			if let Some(key) = &self.admin_key {
				<Admin<T>>::put(key.clone());
			}
			SystemRatio::<T>::put(self.system_ratio);
			TreasuryRatio::<T>::put(self.treasury_ratio);
			OperationRatio::<T>::put(self.operation_ratio);
			MinLiquidationThreshold::<T>::put(self.min_liquidation_threshold);
			ProfitDistributionCycle::<T>::put(self.profit_distribution_cycle);
		}
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// parameters. [blockNumber, blockFee, orderCost, collator]
		/// block has been handled include weight and fee
		BlockProcessed(BlockNumberFor<T>, BalanceOf<T>, Balance, T::AccountId),

		/// liquidation and distribution finished
		DistributionFinished,

		/// parameters. [baseAccount, systemAccount, balance]
		///transfer total block fee from BaseAccount to SystemAccount
		TransferBaseToSystem(T::AccountId, T::AccountId, BalanceOf<T>),

		///parameters. [totalFee, totalCost]
		/// profit distributed succeed
		ProfitDistributed(Balance, Balance),

		///parameters. [totalFee, totalCost]
		/// collators compensated
		CollatorsCompensated(Balance, Balance),

		/// set admin(account_id)
		SetAdmin(T::AccountId),

		/// Set system ratio
		SystemRatioSet(Perbill),

		/// Set treasury ratio
		TreasuryRatioSet(Perbill),

		/// Set operation ratio
		OperationRatioSet(Perbill),

		/// Set min liquidation threshold
		MinLiquidationThresholdSet(Balance),

		/// Set profit distribution cycle
		ProfitDistributionCycleSet(BlockNumberFor<T>),

		/// error occurred
		Error(Error<T>),
	}

	#[pallet::error]
	#[derive(Clone, PartialEq, Eq)]
	pub enum Error<T> {
		/// get real gas cost failed
		FailedToFetchRealGasCost,

		/// internal errors
		TransferBaseToSystemError,

		///get pot account errors
		GetPotAccountError,

		///failed to process liquidation
		ProcessLiquidationError,

		/// Require admin authority
		RequireAdmin,
		///xcm error
		XcmError,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T>
	where
		T::AccountId: From<AccountId32> + Into<AccountId32>,
		<T as pallet_utility::Config>::RuntimeCall: From<pallet_balances::Call<T>>,
		<T as pallet_balances::Config>::Balance: From<BalanceOf<T>>,
		T: pallet_xcm::Config,
	{
		fn on_finalize(n: BlockNumberFor<T>) {
			let base_account = <T::AccountId>::from(BASE_ACCOUNT);
			let base_account_balance = <T as pallet::Config>::Currency::free_balance(&base_account);

			let (collator, real_gas_cost) = match T::OrderGasCost::gas_cost(n) {
				Ok(cost) => match cost {
					Some((collator, real_gas_cost)) => (collator, real_gas_cost),
					None => return,
				},
				Err(_) => {
					Self::deposit_event(Event::Error(Error::<T>::FailedToFetchRealGasCost.into()));
					return;
				},
			};

			let reserved_balance: BalanceOf<T> = <T as pallet::Config>::ExistentialDeposit::get()
				.try_into()
				.unwrap_or_else(|_| Zero::zero());

			let block_fee_except_ed = base_account_balance.saturating_sub(reserved_balance);
			let current_block_fee_u128: Balance =
				block_fee_except_ed.try_into().unwrap_or_else(|_| 0);

			TotalCost::<T>::mutate(|cost| *cost = cost.saturating_add(real_gas_cost));
			CollatorRealGasCosts::<T>::mutate(collator.clone(), |cost| {
				*cost = cost.saturating_add(real_gas_cost);
			});

			let mut count = DistributionBlockCount::<T>::get();
			count = count.saturating_add(1u32.into());
			DistributionBlockCount::<T>::put(count);

			let system_account =
				match pallet_pot::Pallet::<T>::ensure_pot(T::SystemAccountName::get()) {
					Ok(account) => account,
					Err(err) => {
						log::error!("get system account err:{:?}", err);
						Self::deposit_event(Event::Error(Error::<T>::GetPotAccountError.into()));
						return;
					},
				};

			match <T as pallet::Config>::Currency::transfer(
				&base_account,
				&system_account,
				block_fee_except_ed.clone(),
				ExistenceRequirement::KeepAlive,
			) {
				Ok(_) => {
					Self::deposit_event(Event::TransferBaseToSystem(
						base_account.clone(),
						system_account.clone(),
						block_fee_except_ed.clone(),
					));
				},
				Err(err) => {
					log::error!("Transfer to system account failed: {:?}", err);
					Self::deposit_event(Event::Error(Error::<T>::TransferBaseToSystemError.into()));
					return;
				},
			}

			TotalIncome::<T>::mutate(|income| {
				*income = income.saturating_add(current_block_fee_u128)
			});

			let min_liquidation_threshold: Balance =
				MinLiquidationThreshold::<T>::get()
					.try_into()
					.unwrap_or_else(|_| 0);
			let profit = TotalIncome::<T>::get().saturating_sub(TotalCost::<T>::get());

			if profit >= min_liquidation_threshold
				&& count % ProfitDistributionCycle::<T>::get() == Zero::zero()
			{
				DistributionBlockCount::<T>::put(BlockNumberFor::<T>::zero());
				match Self::distribute_profit() {
					Ok(_) => {
						Self::deposit_event(Event::DistributionFinished);
					},
					Err(err) => {
						log::error!("process liquidation failed: {:?}", err);
						Self::deposit_event(Event::Error(
							Error::<T>::ProcessLiquidationError.into(),
						));
					},
				}
			}

			Self::deposit_event(Event::BlockProcessed(
				n,
				block_fee_except_ed.clone(),
				real_gas_cost,
				collator,
			));
		}
	}

	impl<T: Config> Pallet<T>
	where
		T::AccountId: From<AccountId32> + Into<AccountId32>,
		<T as pallet_utility::Config>::RuntimeCall: From<pallet_balances::Call<T>>,
		<T as pallet_balances::Config>::Balance: From<BalanceOf<T>>,
		T::XcmSender: SendXcm,
		T: pallet_xcm::Config,
	{
		fn execute_batch_transfers(
			transfers: Vec<(T::AccountId, BalanceOf<T>)>,
		) -> DispatchResultWithPostInfo {
			let system_account =
				match pallet_pot::Pallet::<T>::ensure_pot(T::SystemAccountName::get()) {
					Ok(account) => account,
					Err(err) => {
						log::error!("get system account err:{:?}", err);
						Err(Error::<T>::GetPotAccountError)?
					},
				};

			let mut calls: Vec<<T as pallet_utility::Config>::RuntimeCall> = vec![];

			for (recipient, amount) in transfers {
				if amount.is_zero() {
					continue;
				}

				let transfer_call = pallet_balances::Call::<T>::transfer_allow_death {
					dest: T::Lookup::unlookup(recipient),
					value: amount.into(),
				};

				let utility_call: <T as pallet_utility::Config>::RuntimeCall = transfer_call.into();
				calls.push(utility_call);
			}

			pallet_utility::Pallet::<T>::batch_all(
				frame_system::RawOrigin::Signed(system_account).into(),
				calls,
			)
			.map_err(|err| {
				log::error!("Batch transfer failed: {:?}", err);
				err
			})?;

			Ok(().into())
		}

		fn distribute_profit() -> DispatchResultWithPostInfo {
			let total_income = TotalIncome::<T>::get();
			let total_cost = TotalCost::<T>::get();

			if total_income > total_cost {
				Self::distribute_positive_profit()?;
				Self::deposit_event(Event::ProfitDistributed(
					total_income.clone(),
					total_cost.clone(),
				));
			} else {
				Self::compensate_collators()?;
				Self::deposit_event(Event::CollatorsCompensated(
					total_income.clone(),
					total_cost.clone(),
				));
			}

			let _ = <CollatorRealGasCosts<T>>::clear(u32::max_value(), None);
			TotalIncome::<T>::put(0u128);
			TotalCost::<T>::put(0u128);

			Ok(().into())
		}

		#[cfg(test)]
		pub fn test_distribute_profit() -> DispatchResultWithPostInfo {
			Self::distribute_profit()
		}

		fn distribute_positive_profit() -> DispatchResultWithPostInfo {
			let total_income = TotalIncome::<T>::get();
			let total_cost = TotalCost::<T>::get();
			let total_profit = total_income.saturating_sub(total_cost);

			let treasury_account = pallet_pot::Pallet::<T>::ensure_pot(
				T::TreasuryAccountName::get(),
			)
			.map_err(|err| {
				log::error!("get treasury account err:{:?}", err);
				Error::<T>::GetPotAccountError
			})?;

			let operation_account = pallet_pot::Pallet::<T>::ensure_pot(
				T::OperationAccountName::get(),
			)
			.map_err(|err| {
				log::error!("get maintenance account err:{:?}", err);
				Error::<T>::GetPotAccountError
			})?;

			let system_ratio = SystemRatio::<T>::get();
			let treasury_ratio = TreasuryRatio::<T>::get();
			let operation_ratio = OperationRatio::<T>::get();

			let treasury_amount = treasury_ratio * total_profit / PARACHAIN_TO_RELAYCHAIN_UNIT;
			let operation_amount = operation_ratio * total_profit;
			let system_amount = system_ratio * total_profit;
			let total_collators_profit =
				total_profit.saturating_sub(treasury_amount + operation_amount + system_amount);

			let origin: OriginFor<T> =
				frame_system::RawOrigin::Signed(treasury_account.clone()).into();

			let _send_treasury_profit = Self::send_assets_to_relaychain_treasury(
				origin,
				treasury_account.into(),
				treasury_amount,
			)?;

			let mut transfers = Vec::new();
			/*
			let treasury_account_profit =
				treasury_amount.try_into().unwrap_or_else(|_| Zero::zero());
			transfers.push((treasury_account, treasury_account_profit));
			*/

			let operation_account_profit =
				operation_amount.try_into().unwrap_or_else(|_| Zero::zero());
			transfers.push((operation_account, operation_account_profit));

			for (collator, collator_cost) in CollatorRealGasCosts::<T>::iter() {
				let collator_ratio = Perbill::from_rational(collator_cost, total_cost);
				let collator_profit = collator_ratio * total_collators_profit;

				let collator_addr_profit =
					collator_profit.try_into().unwrap_or_else(|_| Zero::zero());
				let collator_addr_cost = collator_cost.try_into().unwrap_or_else(|_| Zero::zero());

				transfers.push((collator.clone(), collator_addr_profit));
				transfers.push((collator.clone(), collator_addr_cost));
			}

			Self::execute_batch_transfers(transfers)
		}

		fn compensate_collators() -> DispatchResultWithPostInfo {
			let mut transfers = Vec::new();

			for (collator, collator_cost) in CollatorRealGasCosts::<T>::iter() {
				let collator_addr_cost = collator_cost.try_into().unwrap_or_else(|_| Zero::zero());

				transfers.push((collator.clone(), collator_addr_cost));
			}
			Self::execute_batch_transfers(transfers)
		}

		fn send_assets_to_relaychain_treasury(
			origin: OriginFor<T>,
			recipient: AccountId32,
			amount: u128,
		) -> DispatchResult {
			let recipient_account_id = recipient.into();

			let junction = Junction::AccountId32 {
				id: recipient_account_id,
				network: Some(NetworkId::Rococo),
			};
			let arc_junctions = Arc::new([junction]);

			let beneficiary = Location::new(0, X1(arc_junctions));

			let asset =
				Asset { id: AssetId(Location::new(1, Here)), fun: Fungibility::Fungible(amount) };

			let assets = Assets::from(vec![asset]);
			let versioned_assets = VersionedAssets::from(assets);

			match pallet_xcm::Pallet::<T>::reserve_transfer_assets(
				origin,
				Box::new(VersionedLocation::from(Location::parent())),
				Box::new(VersionedLocation::from(beneficiary)),
				Box::new(versioned_assets),
				0,
			) {
				Ok(_) => {
					frame_support::runtime_print!("reserve_transfer_assets executed successfully.");
				},
				Err(e) => {
					log::error!("Error occurred while executing reserve_transfer_assets: {:?}", e);
					Self::deposit_event(Event::Error(Error::<T>::XcmError.into()));
					return Err(Error::<T>::XcmError.into());
				},
			}
			Ok(())
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T>
	{
		#[pallet::call_index(0)]
		#[pallet::weight(Weight::from_parts(10_000, 0) + T::DbWeight::get().writes(1))]
		pub fn set_admin(
			origin: OriginFor<T>,
			new_admin: <T::Lookup as StaticLookup>::Source,
		) -> DispatchResultWithPostInfo {
			let require = match ensure_signed_or_root(origin) {
				Ok(s) if s == Self::admin_key() => true,
				Ok(None) => true,
				_ => false,
			};

			ensure!(require, Error::<T>::RequireAdmin);

			let new_admin = T::Lookup::lookup(new_admin)?;

			Admin::<T>::mutate(|admin| *admin = Some(new_admin.clone()));

			Self::deposit_event(Event::SetAdmin(new_admin));

			Ok(Pays::No.into())
		}

		#[pallet::call_index(1)]
		#[pallet::weight(Weight::from_parts(10_000, 0) + T::DbWeight::get().writes(1))]
		pub fn set_system_ratio(
			origin: OriginFor<T>,
			ratio: Perbill,
		) -> DispatchResultWithPostInfo {
			ensure_root_or_admin::<T>(origin)?;

			SystemRatio::<T>::put(ratio);
			Self::deposit_event(Event::SystemRatioSet(ratio));
			Ok(Pays::No.into())
		}

		#[pallet::call_index(2)]
		#[pallet::weight(Weight::from_parts(10_000, 0) + T::DbWeight::get().writes(1))]
		pub fn set_treasury_ratio(
			origin: OriginFor<T>,
			ratio: Perbill,
		) -> DispatchResultWithPostInfo {
			ensure_root_or_admin::<T>(origin)?;

			TreasuryRatio::<T>::put(ratio);
			Self::deposit_event(Event::TreasuryRatioSet(ratio));
			Ok(Pays::No.into())
		}

		#[pallet::call_index(3)]
		#[pallet::weight(Weight::from_parts(10_000, 0) + T::DbWeight::get().writes(1))]
		pub fn set_operation_ratio(
			origin: OriginFor<T>,
			ratio: Perbill,
		) -> DispatchResultWithPostInfo {
			ensure_root_or_admin::<T>(origin)?;

			OperationRatio::<T>::put(ratio);
			Self::deposit_event(Event::OperationRatioSet(ratio));
			Ok(Pays::No.into())
		}

		#[pallet::call_index(4)]
		#[pallet::weight(Weight::from_parts(10_000, 0) + T::DbWeight::get().writes(1))]
		pub fn set_min_liquidation_threshold(
			origin: OriginFor<T>,
			threshold: Balance,
		) -> DispatchResultWithPostInfo {
			ensure_root_or_admin::<T>(origin)?;

			MinLiquidationThreshold::<T>::put(threshold);
			Self::deposit_event(Event::MinLiquidationThresholdSet(threshold));
			Ok(Pays::No.into())
		}

		#[pallet::call_index(5)]
		#[pallet::weight(Weight::from_parts(10_000, 0) + T::DbWeight::get().writes(1))]
		pub fn set_profit_distribution_cycle(
			origin: OriginFor<T>,
			cycle: BlockNumberFor<T>,
		) -> DispatchResultWithPostInfo {
			ensure_root_or_admin::<T>(origin)?;

			ProfitDistributionCycle::<T>::put(cycle);
			Self::deposit_event(Event::ProfitDistributionCycleSet(cycle));
			Ok(Pays::No.into())
		}
	}

	/// Ensure the origin is either root or admin.
	fn ensure_root_or_admin<T: Config>(origin: OriginFor<T>) -> DispatchResult {
		match ensure_signed_or_root(origin) {
			Ok(s) if s == Pallet::<T>::admin_key() => Ok(()),
			Ok(None) => Ok(()),
			_ => Err(Error::<T>::RequireAdmin.into()),
		}
	}
}
