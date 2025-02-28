// This file is dual licensed under the terms of the Apache License, Version
// 2.0, and the BSD License. See the LICENSE file in the root of this repository
// for complete details.

use foreign_types_shared::ForeignTypeRef;
use pyo3::IntoPy;

use crate::backend::utils;
use crate::buf::CffiBuf;
use crate::error::{CryptographyError, CryptographyResult};
use crate::exceptions;

#[pyo3::prelude::pyfunction]
#[pyo3(signature = (data, password, backend=None, *, unsafe_skip_rsa_key_validation=false))]
fn load_der_private_key(
    py: pyo3::Python<'_>,
    data: CffiBuf<'_>,
    password: Option<CffiBuf<'_>>,
    backend: Option<&pyo3::PyAny>,
    unsafe_skip_rsa_key_validation: bool,
) -> CryptographyResult<pyo3::PyObject> {
    let _ = backend;
    if let Ok(pkey) = openssl::pkey::PKey::private_key_from_der(data.as_bytes()) {
        if password.is_some() {
            return Err(CryptographyError::from(
                pyo3::exceptions::PyTypeError::new_err(
                    "Password was given but private key is not encrypted.",
                ),
            ));
        }
        return private_key_from_pkey(py, &pkey, unsafe_skip_rsa_key_validation);
    }

    let password = password.as_ref().map(CffiBuf::as_bytes);
    let mut status = utils::PasswordCallbackStatus::Unused;
    let pkey = openssl::pkey::PKey::private_key_from_pkcs8_callback(
        data.as_bytes(),
        utils::password_callback(&mut status, password),
    );
    let pkey = utils::handle_key_load_result(py, pkey, status, password)?;
    private_key_from_pkey(py, &pkey, unsafe_skip_rsa_key_validation)
}

#[pyo3::prelude::pyfunction]
#[pyo3(signature = (data, password, backend=None, *, unsafe_skip_rsa_key_validation=false))]
fn load_pem_private_key(
    py: pyo3::Python<'_>,
    data: CffiBuf<'_>,
    password: Option<CffiBuf<'_>>,
    backend: Option<&pyo3::PyAny>,
    unsafe_skip_rsa_key_validation: bool,
) -> CryptographyResult<pyo3::PyObject> {
    let _ = backend;
    let password = password.as_ref().map(CffiBuf::as_bytes);
    let mut status = utils::PasswordCallbackStatus::Unused;
    let pkey = openssl::pkey::PKey::private_key_from_pem_callback(
        data.as_bytes(),
        utils::password_callback(&mut status, password),
    );
    let pkey = utils::handle_key_load_result(py, pkey, status, password)?;
    private_key_from_pkey(py, &pkey, unsafe_skip_rsa_key_validation)
}

#[pyo3::prelude::pyfunction]
fn private_key_from_ptr(
    py: pyo3::Python<'_>,
    ptr: usize,
    unsafe_skip_rsa_key_validation: bool,
) -> CryptographyResult<pyo3::PyObject> {
    // SAFETY: Caller is responsible for passing a valid pointer.
    let pkey = unsafe { openssl::pkey::PKeyRef::from_ptr(ptr as *mut _) };
    private_key_from_pkey(py, pkey, unsafe_skip_rsa_key_validation)
}

fn private_key_from_pkey(
    py: pyo3::Python<'_>,
    pkey: &openssl::pkey::PKeyRef<openssl::pkey::Private>,
    unsafe_skip_rsa_key_validation: bool,
) -> CryptographyResult<pyo3::PyObject> {
    match pkey.id() {
        openssl::pkey::Id::RSA => Ok(crate::backend::rsa::private_key_from_pkey(
            pkey,
            unsafe_skip_rsa_key_validation,
        )?
        .into_py(py)),
        #[cfg(any(not(CRYPTOGRAPHY_IS_LIBRESSL), CRYPTOGRAPHY_LIBRESSL_380_OR_GREATER))]
        openssl::pkey::Id::RSA_PSS => {
            // At the moment the way we handle RSA PSS keys is to strip the
            // PSS constraints from them and treat them as normal RSA keys
            // Unfortunately the RSA * itself tracks this data so we need to
            // extract, serialize, and reload it without the constraints.
            let der_bytes = pkey.rsa()?.private_key_to_der()?;
            let rsa = openssl::rsa::Rsa::private_key_from_der(&der_bytes)?;
            let pkey = openssl::pkey::PKey::from_rsa(rsa)?;
            Ok(
                crate::backend::rsa::private_key_from_pkey(&pkey, unsafe_skip_rsa_key_validation)?
                    .into_py(py),
            )
        }
        openssl::pkey::Id::EC => {
            Ok(crate::backend::ec::private_key_from_pkey(py, pkey)?.into_py(py))
        }
        openssl::pkey::Id::X25519 => {
            Ok(crate::backend::x25519::private_key_from_pkey(pkey).into_py(py))
        }

        #[cfg(all(not(CRYPTOGRAPHY_IS_LIBRESSL), not(CRYPTOGRAPHY_IS_BORINGSSL)))]
        openssl::pkey::Id::X448 => {
            Ok(crate::backend::x448::private_key_from_pkey(pkey).into_py(py))
        }

        openssl::pkey::Id::ED25519 => {
            Ok(crate::backend::ed25519::private_key_from_pkey(pkey).into_py(py))
        }

        #[cfg(all(not(CRYPTOGRAPHY_IS_LIBRESSL), not(CRYPTOGRAPHY_IS_BORINGSSL)))]
        openssl::pkey::Id::ED448 => {
            Ok(crate::backend::ed448::private_key_from_pkey(pkey).into_py(py))
        }
        openssl::pkey::Id::DSA => Ok(crate::backend::dsa::private_key_from_pkey(pkey).into_py(py)),
        openssl::pkey::Id::DH => Ok(crate::backend::dh::private_key_from_pkey(pkey).into_py(py)),

        #[cfg(all(not(CRYPTOGRAPHY_IS_LIBRESSL), not(CRYPTOGRAPHY_IS_BORINGSSL)))]
        openssl::pkey::Id::DHX => Ok(crate::backend::dh::private_key_from_pkey(pkey).into_py(py)),
        _ => Err(CryptographyError::from(
            exceptions::UnsupportedAlgorithm::new_err("Unsupported key type."),
        )),
    }
}

#[pyo3::prelude::pyfunction]
fn load_der_public_key(
    py: pyo3::Python<'_>,
    data: CffiBuf<'_>,
    backend: Option<&pyo3::PyAny>,
) -> CryptographyResult<pyo3::PyObject> {
    let _ = backend;
    load_der_public_key_bytes(py, data.as_bytes())
}

pub(crate) fn load_der_public_key_bytes(
    py: pyo3::Python<'_>,
    data: &[u8],
) -> CryptographyResult<pyo3::PyObject> {
    match cryptography_key_parsing::spki::parse_public_key(data) {
        Ok(pkey) => public_key_from_pkey(py, &pkey, pkey.id()),
        // It's not a (RSA/DSA/ECDSA) subjectPublicKeyInfo, but we still need
        // to check to see if it is a pure PKCS1 RSA public key (not embedded
        // in a subjectPublicKeyInfo)
        Err(e) => {
            // Use the original error.
            let pkey =
                cryptography_key_parsing::rsa::parse_pkcs1_public_key(data).map_err(|_| e)?;
            public_key_from_pkey(py, &pkey, pkey.id())
        }
    }
}

#[pyo3::prelude::pyfunction]
fn load_pem_public_key(
    py: pyo3::Python<'_>,
    data: CffiBuf<'_>,
    backend: Option<&pyo3::PyAny>,
) -> CryptographyResult<pyo3::PyObject> {
    let _ = backend;
    let p = pem::parse(data.as_bytes())?;
    let pkey = match p.tag() {
        "RSA PUBLIC KEY" => cryptography_key_parsing::rsa::parse_pkcs1_public_key(p.contents())?,
        "PUBLIC KEY" => cryptography_key_parsing::spki::parse_public_key(p.contents())?,
        _ => return Err(CryptographyError::from(pem::PemError::MalformedFraming)),
    };
    public_key_from_pkey(py, &pkey, pkey.id())
}

fn public_key_from_pkey(
    py: pyo3::Python<'_>,
    pkey: &openssl::pkey::PKeyRef<openssl::pkey::Public>,
    id: openssl::pkey::Id,
) -> CryptographyResult<pyo3::PyObject> {
    // `id` is a separate argument so we can test this while passing something
    // unsupported.
    match id {
        openssl::pkey::Id::RSA => Ok(crate::backend::rsa::public_key_from_pkey(pkey).into_py(py)),
        openssl::pkey::Id::EC => {
            Ok(crate::backend::ec::public_key_from_pkey(py, pkey)?.into_py(py))
        }
        openssl::pkey::Id::X25519 => {
            Ok(crate::backend::x25519::public_key_from_pkey(pkey).into_py(py))
        }
        #[cfg(all(not(CRYPTOGRAPHY_IS_LIBRESSL), not(CRYPTOGRAPHY_IS_BORINGSSL)))]
        openssl::pkey::Id::X448 => Ok(crate::backend::x448::public_key_from_pkey(pkey).into_py(py)),

        openssl::pkey::Id::ED25519 => {
            Ok(crate::backend::ed25519::public_key_from_pkey(pkey).into_py(py))
        }
        #[cfg(all(not(CRYPTOGRAPHY_IS_LIBRESSL), not(CRYPTOGRAPHY_IS_BORINGSSL)))]
        openssl::pkey::Id::ED448 => {
            Ok(crate::backend::ed448::public_key_from_pkey(pkey).into_py(py))
        }

        openssl::pkey::Id::DSA => Ok(crate::backend::dsa::public_key_from_pkey(pkey).into_py(py)),
        openssl::pkey::Id::DH => Ok(crate::backend::dh::public_key_from_pkey(pkey).into_py(py)),

        #[cfg(all(not(CRYPTOGRAPHY_IS_LIBRESSL), not(CRYPTOGRAPHY_IS_BORINGSSL)))]
        openssl::pkey::Id::DHX => Ok(crate::backend::dh::public_key_from_pkey(pkey).into_py(py)),

        _ => Err(CryptographyError::from(
            exceptions::UnsupportedAlgorithm::new_err("Unsupported key type."),
        )),
    }
}

pub(crate) fn create_module(py: pyo3::Python<'_>) -> pyo3::PyResult<&pyo3::prelude::PyModule> {
    let m = pyo3::prelude::PyModule::new(py, "keys")?;

    m.add_function(pyo3::wrap_pyfunction!(load_pem_private_key, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(load_der_private_key, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(load_der_public_key, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(load_pem_public_key, m)?)?;

    m.add_function(pyo3::wrap_pyfunction!(private_key_from_ptr, m)?)?;

    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::public_key_from_pkey;

    #[test]
    fn test_public_key_from_pkey_unknown_key() {
        pyo3::prepare_freethreaded_python();

        pyo3::Python::with_gil(|py| {
            let pkey =
                openssl::pkey::PKey::public_key_from_raw_bytes(&[0; 32], openssl::pkey::Id::X25519)
                    .unwrap();
            // Pass a nonsense id for this key to test the unsupported
            // algorithm path.
            assert!(public_key_from_pkey(py, &pkey, openssl::pkey::Id::CMAC).is_err());
        });
    }
}
