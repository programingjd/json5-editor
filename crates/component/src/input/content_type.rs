use gpui::Window;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputContentType {
    Name,

    NamePrefix,

    GivenName,

    MiddleName,

    FamilyName,

    NameSuffix,

    Nickname,

    JobTitle,

    OrganizationName,

    Location,

    FullStreetAddress,

    StreetAddressLine1,

    StreetAddressLine2,

    AddressCity,

    AddressState,

    AddressCityAndState,

    Sublocality,

    CountryName,

    PostalCode,

    TelephoneNumber,

    EmailAddress,

    Url,

    CreditCardNumber,

    CreditCardName,

    CreditCardGivenName,

    CreditCardMiddleName,

    CreditCardFamilyName,

    CreditCardSecurityCode,

    CreditCardExpiration,

    CreditCardExpirationMonth,

    CreditCardExpirationYear,

    CreditCardType,

    Username,

    Password,

    NewPassword,

    OneTimeCode,

    ShipmentTrackingNumber,

    FlightNumber,

    DateTime,

    Birthdate,

    BirthdateDay,

    BirthdateMonth,

    BirthdateYear,

    CellularEid,

    CellularImei,
}

impl InputContentType {
    #[cfg(target_os = "macos")]
    pub(crate) const fn ns_text_content_type(self) -> Option<&'static str> {
        match self {
            Self::Name => Some("name"),
            Self::NamePrefix => Some("honorific-prefix"),
            Self::GivenName => Some("given-name"),
            Self::MiddleName => Some("additional-name"),
            Self::FamilyName => Some("family-name"),
            Self::NameSuffix => Some("honorific-suffix"),
            Self::Nickname => Some("nickname"),
            Self::JobTitle => Some("organization-title"),
            Self::OrganizationName => Some("organization"),
            Self::Location => Some("location"),
            Self::FullStreetAddress => Some("street-address"),
            Self::StreetAddressLine1 => Some("address-line1"),
            Self::StreetAddressLine2 => Some("address-line2"),
            Self::AddressCity => Some("address-level2"),
            Self::AddressState => Some("address-level1"),
            Self::AddressCityAndState => Some("address-level1+2"),
            Self::Sublocality => Some("address-level3"),
            Self::CountryName => Some("country-name"),
            Self::PostalCode => Some("postal-code"),
            Self::TelephoneNumber => Some("tel"),
            Self::EmailAddress => Some("email"),
            Self::Url => Some("url"),
            Self::CreditCardNumber => Some("cc-number"),
            Self::CreditCardName => Some("cc-name"),
            Self::CreditCardGivenName => Some("cc-given-name"),
            Self::CreditCardMiddleName => Some("cc-additional-name"),
            Self::CreditCardFamilyName => Some("cc-family-name"),
            Self::CreditCardSecurityCode => Some("cc-csc"),
            Self::CreditCardExpiration => Some("cc-exp"),
            Self::CreditCardExpirationMonth => Some("cc-exp-month"),
            Self::CreditCardExpirationYear => Some("cc-exp-year"),
            Self::CreditCardType => Some("cc-type"),
            Self::Username => Some("username"),
            Self::Password => Some("password"),
            Self::NewPassword => Some("new-password"),
            Self::OneTimeCode => Some("one-time-code"),
            Self::ShipmentTrackingNumber => Some("shipment-tracking-number"),
            Self::FlightNumber => Some("flight-number"),
            Self::DateTime => Some("date-time"),
            Self::Birthdate => Some("bday"),
            Self::BirthdateDay => Some("bday-day"),
            Self::BirthdateMonth => Some("bday-month"),
            Self::BirthdateYear => Some("bday-year"),
            Self::CellularEid | Self::CellularImei => None,
        }
    }
}

pub(super) fn sync_native_content_type(
    window: &mut Window,
    content_type: Option<InputContentType>,
    editable: bool,
) {
    if !editable {
        return;
    }

    #[cfg(all(target_os = "macos", not(test)))]
    gpui_base::input::set_text_content_type(
        window,
        content_type.and_then(InputContentType::ns_text_content_type),
    );

    #[cfg(any(not(target_os = "macos"), test))]
    let _ = (window, content_type);
}
